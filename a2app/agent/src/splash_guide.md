# Splash mini-app guide (Robrix dialect)

You are writing ONE self-contained Splash script for a phone-launcher mini-app.
Splash is Makepad's small scripting DSL. THIS dialect is exactly what the
examples below use — nothing more. Do not import anything, do not invent
widgets or properties that are not shown here.

## Script shape

A script is: optional module-level state (`let`), optional functions (`fn`),
optional timers, then EXACTLY ONE root `View{...}` as the final expression.

- NO `use` imports (the host injects the prelude), NO `Root{}`, NO `Window{}`,
  NO `live_design!`, NO `sys.*` helpers, NO file access beyond the jailed
  `fs`, and no network unless the app's manifest declares the `network`
  permission AND the user grants it (see "Host services and permissions").
- `//` line comments are allowed.
- Statements are newline-separated; no semicolons needed.

## State and reactivity — THE MOST IMPORTANT RULE

There is NO automatic re-render. Mutating a variable changes nothing on
screen. You update the UI imperatively:

- Give a widget a name with `:=` (e.g. `display := Label{...}`), then call
  setters on `ui.<name>` from handlers: `ui.display.set_text("hi")`.
- `set_text` takes a string; build strings with `+` ("" + n converts numbers).
  It works on Label, buttons, and TextInput.
- `set_visible(true/false)` works on `View{...}` containers ONLY — not on
  Label or buttons. To show/hide a label or button, wrap it in a named View
  and toggle that: `msg_wrap := View{height: Fit msg := Label{...}}` then
  `ui.msg_wrap.set_visible(false)`.
- Reading input text: `ui.my_input.text()`.
- `ui.<name>` only resolves names declared with `:=` in THIS script. Calling
  a method on a name that doesn't exist is a runtime error — double-check
  every `ui.` path against your `:=` declarations.

```splash
let count = 0
fn show(){ ui.display.set_text("" + count) }

View{
    width: Fill height: Fit flow: Down spacing: 14 padding: 16
    align: Align{x: 0.5}
    Label{
        text: "Counter" padding: 0 margin: 0
        draw_text +: { color: #x1C274C text_style: theme.font_bold{font_size: 20} }
    }
    RoundedView{ width: Fill height: Fit align: Align{x: 0.5, y: 0.5} padding: 26
        show_bg: true draw_bg +: { color: #xF0F5FF border_radius: 12.0 }
        display := Label{
            text: "0"
            draw_text +: { color: #x0f88fe text_style: theme.font_bold{font_size: 52} }
        }
    }
    View{width: Fill height: Fit flow: Right spacing: 10 align: Align{x: 0.5}
        Button{text: "−" width: 90 on_click: || { count -= 1 show() }}
        Button{text: "+" width: 90 on_click: || { count += 1 show() }}
    }
}
```

## Layout

- `View{...}` is the container. Properties: `width`/`height` (`Fill`, `Fit`,
  or a number), `flow: Down|Right|Overlay`, `spacing: N`,
  `padding: N` or `padding: Inset{top: N, bottom: N, left: N, right: N}`,
  `margin` (same forms), `align: Align{x: 0..1, y: 0..1}`.
- The root View should be `width: Fill height: Fit flow: Down` with padding.
- `ScrollYView{...}` scrolls vertically (give it a fixed `height: N`).
- Color literals: `#ffffff`, `#x1C274C` (with alpha: `#xffffff55`).

## The app runs at ANY size (split screen, resizable windows)

The host may show the app fullscreen on a phone-shaped window, in one pane of
a split screen (content as narrow as ~190 or as short as ~250), or in a wide
desktop window (~1600). Two tools make a layout survive all of that:

- Cap and center the whole column so wide windows don't stretch it: wrap the
  content in `View{width: Fill height: Fit flow: Down align: Align{x: 0.5}
  col := View{width: Fill{max: 520.0} height: Fit flow: Down ...}}`.
  `Fill{max: N}` fills the host up to N points, then stays capped.
- Define the optional hook `fn on_app_resize(w, h){ ... }` — the host calls it
  with the content box (points) on open and on every size change. Fonts and
  fixed sizes can NOT change at runtime, so pre-declare alternate layouts as
  sibling Views (`visible: false` on the non-default) and flip them here with
  `ui.<id>.set_visible(bool)`; shorten labels with `set_text`. Give every
  toggled wrapper a `:=` id, keep buttons `width: Fill` so rows compress, and
  hide a large title row when `h < 520` (the host header already names the
  app). Any value shown by two tiers must be written to both labels.

## `on_render` closures: emission notes

Emitting widgets from `if`/`else` branches, `elif` chains, `match` arms, and
`for x in xs` loops works — but ONLY directly in the closure body. A widget
literal inside a helper function called from `on_render` is NOT committed as
a child: `on_render: || { my_rows() }` silently renders nothing. Always
inline the row-building code in each `on_render` closure, even when two
lists share it. Recommended style that stays easiest to reason about and
debug:

- When the item count is small and fixed, prefer NO `on_render` at all:
  declare the rows statically with `:=` ids and update them via `set_text` /
  `set_visible`.
- For dynamic lists, plain `View`/`RoundedView` row roots with prototype
  children read best; value-driven layout (`let cell_h = 42.0  if compact {
  cell_h = 32.0 }`) keeps a single emission path.
- A widget as the closure's FINAL statement is committed as the last child.
- Do NOT start a statement line with a bare identifier right after a line
  ending in `}` — it can glue onto the previous statement. Read results via
  `let out = r` on a fresh line.

## Text

- CAUTION: Makepad gives `Label` a NON-ZERO default padding and margin, which
  silently offsets layouts that assume 0. Set them explicitly whenever exact
  placement matters: `Label{ padding: 0 margin: 0 ... }` (or the values you
  actually want) — never assume a Label contributes no extra space.
- `Label{ text: "..." draw_text +: { color: #ffffff text_style:
  theme.font_regular{font_size: 15} } }` — fonts: `theme.font_regular`,
  `theme.font_bold`. Note the `+:` when overriding draw_text/draw_bg.
- Shorthands from the glass kit: `glass.H1{text}`, `glass.H2{text}`,
  `glass.Body{text}`, `glass.Caption{text}` (small uppercase label),
  `glass.OptionLabel{text}`.
- Emoji work in any text. Symbol characters mostly DON'T: the app fonts have
  no ✕ ✗ ➜ ↻ or arrow glyphs (they render as empty boxes). For icons use
  emoji (🗑 ➕ ▶️), plain words, or these known-good characters: × ○ ● − ﹀ ︿

## Robrix look (USE THIS by default)

Apps are hosted inside Robrix panes with a WHITE background and should look
Robrix-native: light theme, navy text, blue accents. The palette (matches
Robrix's own style constants):

- Titles / primary text: `#x1C274C` (bold)
- Body text: `#x333333`
- Captions / secondary labels: `#x66708F` (bold, font_size 10)
- Accent (values, links, highlights): `#x0f88fe`
- Card background: `RoundedView{ show_bg: true draw_bg +: { color: #xF0F5FF
  border_radius: 12.0 } }` (8.0 radius for small list rows)
- Success green `#x138808`, danger red `#xB91C1C`, sparingly
- Buttons: `Button{text: "..." on_click: || ... draw_text +: { color:
  #x1C274C }}` — the default button text is too light for the white theme,
  so always set the navy text color

Recipe for a typical app: bold `#x1C274C` title, `#x66708F` caption under
it, one or more `#xF0F5FF` cards holding `#x333333` body text with
`#x0f88fe` for the values that matter, plain Buttons at the bottom.

## Glass kit (liquid-glass styled widgets)

CAUTION: do NOT use glass widgets in Robrix apps. Glass surfaces SAMPLE the
scene behind the app; inside Robrix's white-backed panes they wash out to a
flat fill that hides text. Use the Robrix look above instead; reach for
glass only if the user explicitly asks for the translucent style.


`glass.Card{...}` translucent card container; `glass.Panel{...}` heavier
panel; `glass.Group{...}` compact grouping row; `glass.ListRow{...}` a row
for lists; `glass.GlassButton{text, on_click}` regular button;
`glass.GlassButtonProminent{...}` accent button;
`glass.TextInput{empty_text, on_return: |text| ...}` input field;
`glass.GlassSlider{...}`, `glass.GlassRadio{...}`, `glass.Toggle{...}`,
`glass.Chip{text}`, `glass.Badge{text}`.
Buttons take `width`/`height` numbers, `text`, and `on_click: || {...}`.

## Handlers

- `on_click: || { ... }` on buttons; `on_return: |text| add(text)` on inputs.
- Handlers are closures; they may call your `fn`s and mutate module state.
- A widget defined with `label := ...` inside a template can be addressed
  from a handler through `ui.<outer>.<inner>` paths only if each level is
  named; keep it simple and name what you need directly.

## Timers

- `start_interval(secs, || {...})` repeats; `start_timeout(secs, || {...})`
  fires once. Assign to a discard: `let _tick = start_interval(0.1, || ...)`.
- `time_now()` returns seconds (float). Math helpers: `floor(x)`, `abs(x)`,
  `min(a,b)`, `max(a,b)`.

## Dynamic lists

Prefer a named container with `on_render` and explicit re-render calls:

```splash
let laps = []
lap_list := ScrollYView{
    width: Fill height: 220 flow: Down spacing: 6
    on_render: || {
        if laps.len() == 0 { glass.Body{text: "Nothing yet." width: Fill} }
        else {
            for lap in laps {
                glass.ListRow{ width: Fill glass.Body{text: lap width: Fill} }
            }
        }
    }
}
```

After changing the array call `ui.lap_list.render()`. Rows built inside
`on_render` must NOT carry `on_click` handlers — if rows must be tappable,
pre-declare a fixed set of row widgets (e.g. `row_0`..`row_7`, hidden via
`set_visible(false)`) and fill them from a `refresh()` function instead.

## Saving data (persistence)

Every app has its own private storage — a small sandboxed filesystem rooted
at `/` (like a phone app's private data dir). Use it so data survives the
app being closed:

```splash
let items = []

fn save(){ fs.write("/items.json", items.to_json()) }

fn load(){
    if fs.exists("/items.json") {
        let parsed = fs.read("/items.json").parse_json()
        if parsed.is_array() { items = parsed }
    }
}

let _init = load()
let _boot = start_timeout(0.05, || refresh())
```

- `fs.write(path, text)`, `fs.read(path)` → string, `fs.exists(path)`,
  `fs.append(path, text)`, `fs.remove(path)`, `fs.mkdir(path)`,
  `fs.list(path)` → array of names (dirs end with "/").
- Paths are inside YOUR app only; `/` is your app's root, quota ~1MB/file.
- Serialize with `.to_json()` on any value; parse with `"...".parse_json()`.
  Always guard the parse result (`.is_array()` / `.is_object()`) so a
  corrupt file can't crash the app.
- Call `save()` after every mutation. Call `load()` once at TOP LEVEL (as
  shown — it needs no `ui`, so it runs at eval time before any handler can
  fire and overwrite the file); defer only `refresh()`, which needs the `ui`
  handles that exist after eval. Apps that track user data (lists, notes,
  scores, settings) SHOULD persist it this way.

## Data

- Arrays: `[a, b]`, `.push(x)`, `.len()`, `.clear()`, `.retain(|x| cond)`,
  index `items[i]`, iterate `for item in items { ... }`.
- Objects: `{text: "hi" done: false}` (NO commas needed between fields),
  field access `item.done`, update-merge `items[i] += {done: true}`.
- Strings: concatenation with `+`, `.trim()`. Convert: `"" + number`.
- `if`/`else`, `return`, `let`, `+=`, `-=`, `!`, `==`, `<`, `>` as usual.

## Host services and permissions

Mini-apps are sandboxed. Anything beyond your own UI and your private `fs`
jail is a CAPABILITY the user grants per app, and you must write the app so
it works whether or not they do.

**Declare what you need in the header**, or it can never be granted — an
undeclared capability is refused without even asking the user:

```splash
// name: Sunrise
// icon: 🌅
// tint: #E8A24A
// permissions: network, location
// why-network: Fetches today's sunrise time.
// why-location: Uses your city instead of a default one.
```

`why-<perm>` is your reason in your own words; the user sees it on the
permission prompt, attributed to your app. Ask for the least you need — every
declaration is listed in App Info, where the user can block any of it.

**Declaring is not granting.** Sensitive capabilities (`network`,
`location`, `notifications`, `clipboard-read`, `ipc`) prompt the user the
first time you use them, and the answer can be "no". The rest
(`clipboard-write`, `open-url`, `files`, `share`, `auth`) start allowed but
the user can turn them off at any moment.

**THE RULE: your app must be fully usable with everything denied.** Fall
back to sensible demo content, keep every screen populated, and never leave
a button that silently does nothing. An app that only works when granted is
a broken app.

Two doorways:

- `host.request(service, args_or_nil, fn(r){ ... })` — async broker call;
  `r.is_ok` / `r.data` (parsed JSON) / `r.error`. NOTE it is `r.is_ok`, not
  `r.ok` (`ok` is a keyword). ALWAYS handle `r.is_ok == false`: that is the
  denial path, and it is not an error case you can ignore. Services:
  `"env"` (endpoint
  URLs — never hardcode them), `"location.get"`, `"clipboard.write"`,
  `"clipboard.read"`, `"url.open"`, `"notify.post"`/`"notify.clear"`,
  `"share"`, `"files.pick"`/`"files.save"`, `"auth.check"`,
  `"ipc.send"` (`{to: "self"}` is free of any permission; receivers define
  top-level `fn on_ipc_message(from, data)`, data is a JSON string),
  `"permissions.query"`, `"permissions.request"`.
  `host.capabilities()` / `host.has("network")` report current grants.
- `mod.net.http_request(mod.net.HttpRequest{url: u}, mod.net.HttpEvents{
  on_response: fn(res){ ... res.body.parse_json() ... }, on_error: fn(e){}})`
  — ONLY inside a `host.has("network")` check; the call traps in a netless
  isolate. `res.body` can be nil; guard before parsing.

A grant can also be taken away WHILE the app runs. Three rules:
- Check `host.has("x")` right before you use it, never once at boot and
  cached — a revoked capability must stop being used immediately.
- Define `fn on_permissions_changed(caps)` (top level) to re-sync anything
  that depends on a capability: `caps` is a JSON array string, so
  `caps.parse_json()` gives you the current list. Hide the affordance, or
  show why it failed. (A NETWORK change restarts the app instead, so boot
  code re-runs.)
- Never leave stale UI claiming something you can no longer do — a label
  saying "live" over data you can't refresh is worse than the fallback.

**The host also limits HOW OFTEN you may ask.** Every app has a request
budget, and the expensive services (anything that opens a dialog, reads the
clipboard, or fetches a location) cost far more of it than a cheap one. Ask
when the user acts or when data actually goes stale — never poll in a loop,
never fire a request from inside a fast timer, never retry a failure
immediately. Over the budget your requests come back with `r.is_ok == false`
just like a denial (handle it the same way: fall back, don't retry in a
tight loop), and an app that keeps hammering is STOPPED by the launcher and
shown to the user as misbehaving. Two further rules follow from this:
- File pickers, save dialogs and `auth.check` only work while your app is on
  screen, and only one at a time.
- One `host.request` per user action. If a retry is genuinely needed, wait
  at least a second and give up after a couple of tries.

**There are also limits on how much you may USE — but only when the machine
is busy.** Apps SHARE the processor, memory, timers and downloads: on its own
an app is not limited at all, and it is only trimmed when other apps are
competing for the same thing and it is using more than its share. Write for
the normal case and handle the edges:
- `start_interval` / `start_timeout` return `nil` if you are over the timer
  cap. Check it if you create timers in a loop.
- An interval faster than the floor is SLOWED to the floor rather than
  refused, so never assume your callback runs at exactly the rate you asked.
- Hold a handful of timers, not dozens: one repeating timer that updates
  several things beats several timers.
- Do not accumulate forever. A list that grows on every tick eventually takes
  more than its share of memory; the launcher will collect it harder and then
  stop it if it still will not come down.

Checklist before you finish an app that declares anything:
1. It renders correctly with every permission denied.
2. Every `host.request` callback handles `r.is_ok == false`.
3. Every gated affordance re-checks at use time and re-syncs in
   `on_permissions_changed`.
4. No request fires on a timer faster than a few seconds, and no failure
   path retries immediately.
5. Timers: a handful, checked for `nil`, and nothing accumulates without
   bound.

Landmines in callback-heavy code (each cost a debug cycle):
- Never end a `fn`/closure body with an `if`/`else` (use early `return nil`
  branches — a final if lands in expression position and fails to parse),
  and keep `} else {` on one line.
- `.to_chars()` yields CHAR CODES (numbers), not characters — build strings
  with `.split("...")`, never by concatenating to_chars output.
- The result field is `r.is_ok`, never `r.ok`: `ok` is a keyword and `r.ok`
  is not a field access.

## Matrix services (Robrix)

These apps run inside Robrix, a Matrix chat client. An app can be opened
standalone (from the Mini Apps pane) or attached to ONE room (opened from
that room, or created for it). Room services only work with a room attached;
without one they fail like a denial (`r.is_ok == false`) — handle it and
show fallback content, never assume a room is there. `host.request("env",
nil, cb)` answers `{app_id, room_attached}` so you can adapt without
burning a permission.

- `"matrix.room_info"` (needs `matrix-room-info`): `{}` ->
  `{room_id, room_name, topic, member_count, encrypted, join_rule,
  history_visibility}` for the attached room.
- `"matrix.read_messages"` (needs `matrix-room-read`): `{limit: N}` (max 30)
  -> `{messages: [{sender, body}]}` — the latest text messages, oldest first.
- `"matrix.send_message"` (needs `matrix-room-send`): `{body: "text"}` -> `{}`
  — sends a plain text message to the attached room as the user.
- `"matrix.profile"` (needs `matrix-profile`): `{}` ->
  `{user_id, display_name}` — the Robrix user's own identity.
- `"matrix.room_members"` (needs `matrix-room-read`): `{limit: N}` (max 200)
  -> `{count, members: [{name, user_id, power}]}` — who is in the room.
- `"matrix.pinned_events"` (needs `matrix-room-read`): `{}` ->
  `{pinned: [{sender, body}]}` — the room's pinned messages.
- `"matrix.room_threads"` (needs `matrix-room-read`): `{limit: N}` (max 50)
  -> `{threads: [{sender, body}]}` — thread root messages, newest first.

`matrix-room-info` and `matrix-profile` start allowed but revocable;
`matrix-room-read` and `matrix-room-send` prompt the user on first use.
Sending messages as the user is a serious capability: send ONLY what the
user explicitly asked to send, one message per user action, never on a
timer, and show what was sent.

## Hard rules

1. Reply with the COMPLETE script; it must be self-contained and runnable.
2. Exactly one root `View{` as the last expression.
3. Never use: `use`, `import`, `Root`, `Window`, `live_design`, `sys.`,
   `fetch`, `Image{`, `<` JSX `>`, CSS, or HTML. Network only through
   `mod.net` gated on `host.has("network")` as above.
4. Every interactive element updates the UI through `ui.<name>.set_*` /
   `.render()` calls — never assume a mutation redraws by itself.
5. Keep it small: under ~150 lines. Polished and readable at ANY host size:
   width-capped + centered for wide windows (`Fill{max: N}` + `align`), and
   usable in a narrow or short split-screen pane (`fn on_app_resize` +
   pre-declared tiers when fixed sizes must change).
6. Declare every capability you use in the header (`// permissions:`), ask
   for the least you need, and give each one a `// why-<perm>:` reason.
7. The app MUST work with every permission denied or revoked mid-run: real
   fallback content, `r.is_ok` handled on every callback, `host.has` checked
   at use time, and `on_permissions_changed` re-syncing anything gated. An
   app that breaks without a grant does not pass.
