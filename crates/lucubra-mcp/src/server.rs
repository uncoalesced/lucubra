// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `LucubraServer`, `SessionRuntime`, blocking I/O bridge, and `rmcp::ServerHandler`.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use lucubra_core::Workspace;
use lucubra_core::state::col;
use lucubra_ingest::worker::{DEFAULT_TIMEOUT, Supervisor, python_supervisor};
use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    Implementation, InitializeRequestParams, InitializeResult, ListPromptsResult, ListToolsResult,
    PaginatedRequestParams, Prompt, ProtocolVersion, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::{Map, Value};
use tokio::sync::{Mutex, RwLock};

use crate::budget::resolve_tier;
use crate::envelope::{ToolError, ToolErrorCode, finish_error, finish_success};
use crate::host::{DetectedCapabilities, DetectedHost, HostKind, detect_host, record_host_profile};
use crate::instructions::SERVER_INSTRUCTIONS;
use crate::prompts;
use crate::tools::{build_tools, dispatch_sync};

/// Options configuring a [`LucubraServer`] instance.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Path to an initialized Lucubra workspace folder (`.lucubra/VERSION`).
    pub workspace_root: PathBuf,
    /// App-data directory holding derived SQLite caches and renders.
    pub data_dir: PathBuf,
    /// When true, write tools refuse mutations with `invalid_state`.
    pub read_only_tools: bool,
}

/// Mutable per-connection session state tracked across MCP requests.
#[derive(Debug, Clone)]
pub struct SessionRuntime {
    /// Detected client host identity and declared capabilities.
    pub host: DetectedHost,
    /// Optional `clientInfo.name` snapshot.
    pub client_name: Option<String>,
    /// Optional `clientInfo.version` snapshot.
    pub client_version: Option<String>,
    /// Negotiated protocol version string.
    pub protocol_version: Option<String>,
    /// Capability flags copied from `initialize`.
    pub detected_caps: DetectedCapabilities,
    /// Cumulative estimated tokens served in this MCP connection.
    pub served_tokens: u64,
    /// Currently open `lucubra_session` id on this connection.
    pub active_session_id: Option<String>,
}

struct ServerInner {
    ws: Arc<Workspace>,
    read_only_tools: bool,
    supervisor: Arc<Mutex<Option<Supervisor>>>,
    runtime: RwLock<SessionRuntime>,
    tools: Vec<Tool>,
    prompts: Vec<Prompt>,
}

/// Thread-safe handle to the Lucubra MCP server implementing [`rmcp::ServerHandler`].
#[derive(Clone)]
pub struct LucubraServer {
    inner: Arc<ServerInner>,
}

const WRITE_TOOLS: &[&str] = &[
    "lucubra_profile",
    "lucubra_ingest",
    "lucubra_submit",
    "lucubra_plan",
    "lucubra_session",
    "lucubra_practice",
];

impl LucubraServer {
    /// Opens an existing workspace and builds a new server handle.
    pub fn open(options: &ServerOptions) -> lucubra_core::Result<Self> {
        let ws = Workspace::open(&options.workspace_root, &options.data_dir)?;
        Ok(Self::from_workspace(ws, options.read_only_tools))
    }

    /// Wraps an already-opened [`Workspace`] in a new server handle.
    pub fn from_workspace(ws: Workspace, read_only_tools: bool) -> Self {
        let default_host = DetectedHost {
            profile: HostKind::Generic.profile(),
            client_name: String::new(),
            client_version: String::new(),
            protocol_version: ProtocolVersion::LATEST_WITH_INITIALIZE.as_str().to_owned(),
            capabilities: DetectedCapabilities::default(),
        };
        let runtime = SessionRuntime {
            host: default_host,
            client_name: None,
            client_version: None,
            protocol_version: None,
            detected_caps: DetectedCapabilities::default(),
            served_tokens: 0,
            active_session_id: None,
        };
        Self {
            inner: Arc::new(ServerInner {
                ws: Arc::new(ws),
                read_only_tools,
                supervisor: Arc::new(Mutex::new(None)),
                runtime: RwLock::new(runtime),
                tools: build_tools(),
                prompts: prompts::list_prompts(),
            }),
        }
    }

    /// Returns a snapshot of the runtime session state.
    pub async fn runtime_snapshot(&self) -> SessionRuntime {
        self.inner.runtime.read().await.clone()
    }

    /// Stops any live Python worker subprocess on shutdown.
    pub async fn shutdown_worker(&self) {
        let mut guard = self.inner.supervisor.lock().await;
        drop(guard.take());
    }

    /// Processes an MCP `initialize` request outside transport framing (useful in tests).
    pub async fn record_client_initialize(
        &self,
        request: InitializeRequestParams,
        negotiated_protocol: &str,
    ) -> Result<DetectedHost, McpError> {
        let detected = detect_host(&request, negotiated_protocol);
        {
            let mut rt = self.inner.runtime.write().await;
            rt.client_name =
                (!detected.client_name.is_empty()).then(|| detected.client_name.clone());
            rt.client_version =
                (!detected.client_version.is_empty()).then(|| detected.client_version.clone());
            rt.protocol_version = Some(detected.protocol_version.clone());
            rt.detected_caps = detected.capabilities;
            rt.host = detected.clone();
        }
        let ws = Arc::clone(&self.inner.ws);
        let det_clone = detected.clone();
        tokio::task::spawn_blocking(move || record_host_profile(&ws, &det_clone))
            .await
            .map_err(|e| McpError::internal_error(format!("initialize task panic: {e}"), None))?
            .map_err(|e| McpError::internal_error(format!("profile.set error: {e}"), None))?;
        Ok(detected)
    }

    /// Executes one `tools/call` directly against the pre-validator and dispatcher.
    pub async fn handle_call_raw(
        &self,
        tool_name: &str,
        raw_args: Option<Map<String, Value>>,
    ) -> Result<CallToolResponse, McpError> {
        let (host, active_session_id, prior_served) = {
            let rt = self.inner.runtime.read().await;
            (
                rt.host.clone(),
                rt.active_session_id.clone(),
                rt.served_tokens,
            )
        };

        if self.inner.read_only_tools && WRITE_TOOLS.contains(&tool_name) {
            let ws = Arc::clone(&self.inner.ws);
            let host_id = host.profile.id.to_owned();
            let host_default = host.profile.default_tier;
            let state = tokio::task::spawn_blocking(move || ws.replay().map(|r| r.state).ok())
                .await
                .unwrap_or(None);
            let limits = resolve_tier(
                state.as_ref().and_then(|s| s.collections.get(col::PROFILE)),
                &host_id,
                host_default,
            )
            .limits();
            let (resp, added) = finish_error(
                ToolError::new(
                    ToolErrorCode::InvalidState,
                    "server is running in read-only mode; write tools are disabled",
                ),
                Vec::new(),
                limits,
                prior_served,
                false,
            );
            self.inner.runtime.write().await.served_tokens += added;
            return Ok(resp);
        }

        let wants_worker = tool_name == "lucubra_ingest"
            && raw_args
                .as_ref()
                .and_then(|m| m.get("action"))
                .and_then(Value::as_str)
                == Some("scan")
            && raw_args
                .as_ref()
                .and_then(|m| m.get("extract"))
                .and_then(Value::as_bool)
                != Some(false);

        let sup_guard = if wants_worker {
            Some(self.inner.supervisor.clone().lock_owned().await)
        } else {
            None
        };

        let tool_owned = tool_name.to_owned();
        let ws = Arc::clone(&self.inner.ws);

        let dispatch = tokio::task::spawn_blocking(move || {
            let mut guard = sup_guard;
            if let Some(ref mut slot) = guard
                && slot.is_none()
                && let Ok(sup) = python_supervisor(DEFAULT_TIMEOUT)
            {
                **slot = Some(sup);
            }
            let sup_ref = guard.as_deref_mut().and_then(|opt| opt.as_mut());
            dispatch_sync(
                &tool_owned,
                raw_args,
                &ws,
                &host,
                active_session_id.as_deref(),
                sup_ref,
            )
        })
        .await
        .map_err(|e| McpError::internal_error(format!("tool worker panic: {e}"), None))?;

        let (response, added) = match dispatch.outcome {
            Ok(outcome) => finish_success(
                outcome.data,
                outcome.next,
                outcome.warnings,
                dispatch.limits,
                outcome.per_call_limit,
                prior_served,
                dispatch.suppress_wrap_up,
            ),
            Err(err) => finish_error(
                err,
                Vec::new(),
                dispatch.limits,
                prior_served,
                dispatch.suppress_wrap_up,
            ),
        };

        let mut rt = self.inner.runtime.write().await;
        rt.served_tokens += added;
        if let Some(next_sid) = dispatch.session_update {
            rt.active_session_id = next_sid;
        }
        Ok(response)
    }
}

impl ServerHandler for LucubraServer {
    fn get_info(&self) -> ServerConfig {
        let caps = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .build();
        ServerConfig::new(caps)
            .with_server_info(
                Implementation::new("lucubra", lucubra_core::VERSION).with_title("Lucubra"),
            )
            .with_instructions(SERVER_INSTRUCTIONS)
            .with_protocol_version(ProtocolVersion::LATEST_WITH_INITIALIZE)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::known_up_to(&ProtocolVersion::V_2025_11_25))
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        let negotiated = self.negotiate_initialize(&request)?;
        context.peer.set_peer_info(request.clone());
        self.record_client_initialize(request, negotiated.protocol_version.as_str())
            .await?;
        Ok(negotiated)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.inner.tools.clone()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.inner.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.handle_call_raw(&request.name, request.arguments).await
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(
            self.inner.prompts.clone(),
        ))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        prompts::expand_prompt(&request)
    }
}
