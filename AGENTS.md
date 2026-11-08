# Agent notes

## UI changes

When a change adds, removes or alters visible UI, show the result inline in the thread before finishing: render every affected screen and state with `flexspeak --snap <file>.png --scene <name>` (`--list-scenes` names them, and a new state gets a new scene), then embed each PNG in your reply with Markdown image syntax and an absolute path. Done means each changed state is visible in the thread, including one that was removed from the UI.
