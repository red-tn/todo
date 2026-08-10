# Tag dropdown in the add/edit form — design

Date: 2026-08-10
Status: approved

## Problem

Existing tags are only surfaced through a native `<datalist>` on the tag input.
In WebView2 the datalist popup is unreliable and only appears after typing, so
existing tags are not discoverable when creating a task. Users re-type (and
mistype) tags instead of reusing them.

## Design

Replace the `<datalist>` with a custom dropdown panel owned by the add form.

### Markup (`src/index.html`)

- Remove `<datalist id="tag-suggestions">`.
- Add `<div class="tag-dd" id="tag-dd" hidden></div>` inside the existing
  `.chip-field`, absolutely positioned under the tag input.

### Behavior (`src/addform.js`)

- Focusing or clicking the tag input opens the dropdown listing all existing
  tags (from current todos, same source as today) minus tags already added to
  the form, sorted alphabetically.
- Typing filters the list live against the normalized input. When nothing
  matches, the dropdown hides; Enter still commits the typed text as a new tag,
  unchanged from today.
- Clicking an item adds it as a chip, clears the input, and keeps the dropdown
  open (refreshed) so several tags can be picked in a row.
- Keyboard: ArrowDown/ArrowUp move a highlight through the list; Enter picks
  the highlighted item, or commits the typed text when nothing is highlighted;
  Escape closes only the dropdown (stopping propagation so it no longer
  collapses the whole form while the dropdown is open).
- Clicking outside the field or leaving it closes the dropdown.
- Add and edit mode share the form, so both get the behavior for free.
- Tag normalization, chip rendering, and the store are untouched.

### Styles (`src/styles.css`)

Panel styled with the existing theme tokens (`--input-bg`, `--input-border`,
`--accent`, `--shadow`), max-height around five rows with overflow scroll.
`.chip-field` becomes `position: relative` to anchor it.

## Testing

No JS test infra exists in this project; verification is manual: open/filter/
pick/keyboard/Escape behavior in both add and edit modes, and free-typed new
tags still work.
