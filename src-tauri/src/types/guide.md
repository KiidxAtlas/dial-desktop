# Building a Type's app

A Type is a small web app made for the things the user keeps in it, not a list of records. You
build the whole app; there are no built-in layouts. Make it something the user is glad to open:
- recipes become a cookbook: browse cards with photos, a recipe page with servings, ingredients to
  tick off and a step-by-step cook mode with timers;
- weather becomes a real, live weather app: current conditions, hourly and 7-day forecast, the
  user's saved cities, fetched fresh each time it opens;
- books become a bookshelf, trips an itinerary with a map-like timeline, habits a streak tracker,
  workouts a log with charts;
- a game becomes a game you can actually play, with its best scores kept.

## How it runs

The app is one HTML document (`html`, up to 1 MB) shown in a sandboxed frame that fills the Type's
canvas (about 700-1400 px wide, full height; it must also work at 360 px). It has an opaque
origin: `localStorage`, cookies, IndexedDB, `alert`, `confirm`, `prompt`, popups and top-level
navigation are unavailable. Build dialogs and confirmations into the page.

Network is https only: `fetch` works with APIs that allow CORS (weather without a key:
`https://api.open-meteo.com/v1/forecast` and `https://geocoding-api.open-meteo.com/v1/search`),
and scripts, styles, fonts and images may load from https CDNs (`https://cdn.jsdelivr.net`,
`https://esm.sh`, `https://fonts.googleapis.com`). Plain JS or ES modules from a CDN; no build
step. Never embed API keys the user did not give you. Handle offline and failed requests with a
clear message and a retry.

The frame is cross-origin, and the system webview throttles `requestAnimationFrame`, CSS
transitions and animations in it until the user first clicks inside it. So never let what the app
shows depend on a transition or animation finishing (set the final state directly; animate only
as decoration), and start game loops and canvas animations from a click ("Click to play").

## Data: `window.dial`

Before your scripts run, the page has `window.dial`:
- `dial.onChange(cb)`: calls `cb(dial)` once the data arrives and after every change (an item
  saved from the chat, an edit, the agent). Render from it; never hard-code the user's data in the
  HTML.
- `dial.type`: `{id, name, description, icon, accent, fields}`.
- `dial.items`: `[{id, created, updated, fields, done}]`, in the user's order. `fields` follows
  the Type's `fields` (a list of `{key, label, kind}`; kinds text, longtext, markdown, list,
  steps, image, images, time `HH:MM`, date `YYYY-MM-DD`, datetime `YYYY-MM-DD HH:MM`, number,
  url, tags, rating 0-5, toggle, json). With no fields, items keep any keys.
- `await dial.add(fields)` → the new item; `await dial.update(id, {fields, done})` (a `null`
  field clears it); `await dial.remove(id)`; `await dial.move(id, index)`.
- `dial.state` and `await dial.setState(object)`: the app's own JSON (settings, selected city,
  game progress, cached results), up to 512 KB. Do not call it from inside `onChange`.
- `dial.image(name)`: a data URL for an image field's value (asset name), or null while it loads.
  `await dial.saveImage(fileOrDataUrl)` stores a picture the user drops or picks and returns the
  name to put in an image field.
- `dial.notify(title, body)`: a system notification. `dial.open(url)`: opens a link in the user's
  browser (plain `<a href="https://...">` links do this by themselves).

Let the user add, edit and remove items in the app itself, not only through the chat. Use
`fields` for what each saved thing is, `state` for everything else.

## Quality bar

- Design for the content: a distinctive look that fits it (a cookbook's warm serif and paper, a
  weather app's sky gradients that follow the conditions, a game's own art). Pick a type scale, a
  palette with good contrast and consistent spacing; use the Type's `accent`.
- Real states: a welcoming empty state that says what to do, loading skeletons, errors.
- Responsive, keyboard-friendly, smooth but restrained motion, `prefers-color-scheme` unless the
  look is the point, `prefers-reduced-motion` respected.
- Games: click or press a key to start, keyboard and pointer controls, pause, restart, score and
  best score in `state`, a steady `requestAnimationFrame` loop, canvas scaled for the device pixel
  ratio. The frame needs a click before it receives keys: say so on the start screen.
- No placeholder text, lorem ipsum or dead buttons. Everything shown works.

## Build and check

1. Choose `fields` and write the app: `types_update` with `changes.html` (or `htmlFile`, a file
   you wrote with your own tools), or `types_create` for a new Type. Set `icon`, `accent` and a
   one-line `description` too.
2. Save what the user gave you with `types_add_item` (images as absolute local paths).
3. If the Type is not on screen, `types_open` it. Then `types_screenshot`: look at it critically
   (alignment, overflow, contrast, empty space, broken images) and read the console errors it
   reports.
4. Exercise it with `types_run` (click the add button, fill the form, start the game, check what
   the DOM shows), and look again.
5. Fix with `types_edit` (exact find/replace, after `types_source`) and repeat 3-5 until it looks
   finished. Never end a turn without having looked at the result.
