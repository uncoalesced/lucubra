# Privacy

Lucubra is local-first software. It runs on your machine, reads the files in the folder you choose, and keeps its state in that folder and in one application-data directory on the same machine.

## What Lucubra does

- Reads the files in your workspace folder, extracts text from supported formats, and indexes that text locally.
- Writes its state to the `.lucubra/` folder inside the workspace and to the application-data directory. Section 4 of [DATA_HANDLING.md](DATA_HANDLING.md) lists each path.
- Returns study material to the AI host that started it, when that host asks through the Model Context Protocol (MCP).

## What Lucubra does not do

- It has no telemetry, no crash reporting and no update check.
- It connects to the internet in two cases only, and only when you ask:
  - It reads the calendar address (iCal) that you supply, to find your busy times. The address stays on this machine, outside the workspace. It never appears in the event log, in tool responses or in error messages.
  - It downloads a file into `inbox/` when you ask, and only from a site listed in `SOURCES.md`. Downloads are off when that file does not exist.

  Neither request carries cookies, workspace content or anything that identifies you.
- It listens on a network port only while `lucubra serve` runs the companion page. The page is bound to `127.0.0.1`, so other machines cannot reach it, and every request needs a fresh token.
- It does not send your files, your event logs or your cache to the developers or to any third party.
- It does not open files that exist only in a cloud folder until you approve it.

## What your AI host does

Lucubra returns study material to your AI host. The host decides what happens next, including whether the material is sent to a model provider in the cloud. Read the privacy policy of your host before you connect it to your study folder. If you need the material to stay on your machine, use a host that runs its model locally.

## Your controls

- Choose which folder is the workspace. Lucubra reads only inside that folder.
- Do not place a workspace inside a synced folder if you do not want the sync provider to receive your files and event logs.
- Inspect, reset or delete your data at any time, as described in [DATA_HANDLING.md](DATA_HANDLING.md) sections 8 to 10.

## Changes

This statement covers the beta release. Features that send or store data more widely will be described here and in [DATA_HANDLING.md](DATA_HANDLING.md) before they ship, and they will be opt-in.

## Contact

Privacy questions go to the maintainer through a GitHub issue, or through private vulnerability reporting for anything sensitive (see [SECURITY.md](SECURITY.md)).
