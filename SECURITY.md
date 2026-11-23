# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 1.0.0-beta.x | Yes. Security fixes land in the next beta |
| 0.0.x (development builds) | No |

## Reporting a vulnerability

Report vulnerabilities privately through GitHub private vulnerability reporting:

1. Open the repository `uncoalesced/lucubra` on GitHub.
2. Select the **Security** tab.
3. Select **Report a vulnerability** and fill in the form.

Do not open a public issue or pull request for a vulnerability. Do not include real study material, personal data or credentials in the report. A minimal file that reproduces the problem is enough.

## Response targets

These are targets for a volunteer-maintained beta, not guarantees.

| Step | Target |
|---|---|
| Acknowledge the report | 3 working days |
| Confirm or reject the finding, with reasons | 10 working days |
| Fix or mitigation for a confirmed high or critical issue | 30 days where practical |
| Public disclosure | After a fix is released, coordinated with the reporter |

## Scope

In scope:

- Ingestion of untrusted files. Lucubra reads files from folders that may come from other people, cloud sync or downloads. Crashes, memory exhaustion, path escapes, zip bombs and code execution through a parser or helper tool are in scope.
- MCP tool inputs. Malformed or hostile arguments from an AI host, including path traversal, reads of the `.lucubra/` folder and injected instructions in study material, are in scope.
- The local loopback surface. The optional HTTP transport is off by default. Any listener that accepts connections from other machines, or that accepts requests without the checks described in the documentation, is in scope.
- Data exposure described in [DATA_HANDLING.md](DATA_HANDLING.md). Any path where Lucubra sends data that the document says it does not send is in scope.

Out of scope:

- The behaviour of AI hosts and model providers. Report those to the vendor.
- Vulnerabilities in third-party libraries that are already published with an advisory. Report them to the upstream project, and tell us if Lucubra needs an update.
- Issues that need full control of the user's account or machine before Lucubra runs.

## Safe harbour

Good-faith research that follows this policy is welcome. Do not access data that is not yours, do not degrade service for other users and do not publish findings before a fix is available.
