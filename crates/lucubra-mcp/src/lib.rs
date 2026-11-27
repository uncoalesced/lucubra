// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Model Context Protocol server: tools, prompts, server instructions, host
//! detection and response budgets.

pub mod budget;
pub mod elicit;
pub mod envelope;
pub mod fetch;
pub mod host;
pub mod instructions;
pub mod material;
pub mod prompts;
pub mod server;
pub mod tools;
pub mod transport;

pub use budget::{BudgetInfo, BudgetLimits, BudgetTier, estimate_tokens};
pub use envelope::{ProgressEntry, ToolError, ToolErrorCode};
pub use host::{DetectedCapabilities, DetectedHost, HostKind, HostProfile};
pub use instructions::{INSTRUCTIONS_VERSION, MAX_INSTRUCTIONS_TOKENS, SERVER_INSTRUCTIONS};
pub use material::MaterialPayload;
pub use server::{LucubraServer, ServerOptions, SessionRuntime};
pub use transport::serve_stdio;
