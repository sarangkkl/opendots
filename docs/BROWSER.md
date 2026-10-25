# Browser capabilities

In Settings, create an agent's saved browser profile. Chrome or Chromium must already be installed; use the executable field if discovery fails. Each agent has one separate profile. Profiles cannot be reassigned because that would expose another agent's logins.

Choose **Log in / take over**, sign in directly in Chrome, then **Resume automation**. Cookies persist in the dedicated profile until the website expires them. OpenDots never imports your everyday Chrome profile. Authentication challenges require manual interaction. Do not put passwords in chat or tool arguments.

Agents launch their configured browser automatically when calling `browser_observe`. If its process has exited, OpenDots finalizes the old capture and relaunches the same saved profile. A restart invalidates pending browser approvals and snapshots; it never retries an action from the old page. Live manual sessions and externally locked profiles are not taken over.

Ask the agent to use `browser_observe`, then `browser_action`. Observations include indexed targets, tabs, viewport coordinates, and optional native screenshot content. Browser interaction approvals appear in the existing approval queue; every interaction asks by default. Agent settings expose per-action allow/ask/deny policies. Allowing clicks can permit posting or submitting forms—there is no reliable generic “publish-only” detector.

Use the execution inspector for live viewing, takeover/resume, screenshots, downloads, video segments, export, deletion, and explicit replay. **Cancel execution** stops the workflow. Closing the app window leaves it in the tray; Quit finalizes capture and shuts down owned Chrome processes.

## Files and group collaboration

Settings → Files available for upload copies an explicitly selected file into managed storage and assigns it to one agent. `browser_artifacts` lists its reference. Agents pass that reference to the upload action; arbitrary filesystem paths are rejected. Downloads use the execution's managed directory and Chrome-generated filenames. A `.crdownload` file is still incomplete.

Execution artifacts use `browser:<execution-id>/<relative-name>` references. Agents may explicitly share those references through `send_to_agent`. A recipient may upload shared media but gets its own Chrome profile and session. Cookies, tabs, private messages, and other unshared artifacts do not follow a handoff.

## Capture and replay

Recording is silent VP8/WebM, up to 10 fps and 1280 pixels wide. Live view and encoding consume one CDP screencast. Fifteen-second segments finalize independently; size changes and tab changes start new segments. Capture timestamps and gaps are preserved within segments. Login/takeover is not captured. Ordinary authenticated page content can appear in recordings.

An interrupted `.partial` segment may be unusable; completed segments remain available after restart. Encoding failures leave action history and screenshots intact, plus a recording-error artifact. The app never launches FFmpeg. Preview uses the system WebView's codec support; export remains available if that WebView cannot play WebM.

Replay creates a new execution, using current policies and fresh observations. Element targets are relocated by recorded role, name, tag and input type; ambiguous or missing targets stop replay. Failed or uncertain prior actions stop replay before dispatch. Coordinate clicks on uniquely identifiable elements preserve their relative position during replay. Coordinates without a recorded target, drag gestures, and ephemeral tab IDs stop for manual relocation rather than guessing. Replay can repeat external actions, so review its approvals carefully.

## Current limits

- This controls browser pages, not native windows or OS dialogs. File inputs use CDP directly.
- Open shadow DOM and same-origin frames are traversed. Cross-origin frames and closed shadow roots can require manual interaction; inaccessible frame targets are reported rather than fabricated.
- Known key names are supported; arbitrary shortcut chords and native file-picker automation are not exposed.
- Sites may block automation or expire logins. No CAPTCHA bypass or dedicated Instagram API integration is included.
- Browser history, recordings and profiles reside on the local device. Use profile/artifact deletion controls to remove them.
- Live provider/platform verification is listed in VALIDATION.md; configured CI is not a completed test run.

Run the native fixture with Chrome installed:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked browser_fixture -- --ignored --nocapture
```

Set `OPENDOTS_TEST_CHROME` to an absolute executable path if necessary. Fixtures use temporary headless profiles and localhost pages, never your social-media accounts.
