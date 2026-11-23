# Accessibility

Lucubra aims to meet the Web Content Accessibility Guidelines (WCAG) 2.1, level AA, for every user interface it provides. Accessibility is part of the release criteria for each feature.

## Current interfaces

| Interface | Status |
|---|---|
| Command-line interface (`lucubra`) | Plain text output, no colour required to read it, no progress animation that replaces content. Output works with screen readers that read the terminal buffer |
| MCP tools used through an AI host | Accessibility depends on the host. Lucubra returns plain text and structured JSON |
| Companion page (planned, milestone M7) | Will be built to WCAG 2.1 AA, with keyboard operation, visible focus, text alternatives and a contrast ratio of at least 4.5 to 1 for body text |
| Desktop extension bundle | Installed and configured by the host. Accessibility depends on the host |

## Design rules

- Text contrast of at least 4.5 to 1 for body text and 3 to 1 for large text.
- No information carried by colour alone.
- Every interactive element reachable by keyboard, with a visible focus indicator.
- Output that does not rely on animation, and does not flash more than three times per second.
- Support for the operating system's text size and reduced-motion settings.
- Plain language in messages, and an error message that states what to do next.

## Known gaps

- The CLI prints long tables that are hard to read on a narrow terminal. Use `--limit` on search, and read output in a wider window.
- The companion page is not yet built. This document will describe it only when it ships.
- Some output of study material is in the language of the source file. Text direction and right-to-left layout depend on the source and the host.

## Reporting an accessibility problem

Open a GitHub issue with the label `accessibility` and include:

- the interface you were using (CLI, host name and version, or companion page);
- your operating system and assistive technology, such as the screen reader and its version;
- the steps you took and what you expected to happen.

Do not include study material in the report. A description of the problem is enough. Accessibility reports are triaged before feature requests.
