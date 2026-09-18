# The RackForge interface theme

Three stylesheets dress the Web interface, and `main.tsx` loads them in this
order. The order is the contract: everything below depends on it.

```ts
import "./design/tokens.css";   // 1. the vocabulary
import "./styles.css";          // 2. structure, and a palette
import "./faceplate.css";       // 3. RackForge's material
```

**`design/tokens.css`** is the vocabulary with no opinion about colour:
spacing (`--rf-space-*`), radii, touch targets (`--rf-target-*`), motion
durations and curves, elevation, layers. Reach for these before writing a
number into a rule.

**`styles.css`** lays out every screen and defines the colour tokens
(`--acid`, `--ink`, `--line`, `--red`…), including the dark palette.

**`faceplate.css`** is what RackForge looks like: a hardware panel, with
seams, lips, wells and keys. It restyles what `styles.css` laid out.

## What actually paints a control

The faceplate wins, and it wins by `!important` — 1128 of them. That is the
mechanism today, not an accident to be tidied away one declaration at a
time, and it has a consequence worth stating plainly:

> **A colour declared in `styles.css` for a selector the faceplate forces is
> read by people and by nothing else.**

There are 887 such declarations. They are not a bug in themselves, but they
are a trap: reading `styles.css` alone tells you the primary key is filled
with `var(--acid)`, which is a blue; opening the app shows it red, because
`faceplate.css` forces `var(--red)`. `src/theme.test.ts` records that count
so the trap does not get deeper.

That 887 is a floor, not the total. It counts only declarations whose
selector the faceplate forces *exactly*, and `!important` does not stop at
matching selectors — it outranks every normal declaration that reaches the
same element, however specific. `.pairing-panel .primary-button:disabled`
was more specific than `.primary-button` and still lost, which is why a
disabled key rendered as an armed one until the faceplate was given a
`:disabled` state of its own. A rule that looks safe because it is specific
is not safe.

## Showing that a control is off

The faceplate owns this too, and it has two ways of doing it. The touch keys
dim with `opacity: 0.42`, which suits a key in a row of keys. A key standing
alone on a panel names the inert tokens instead — `--muted` ink on
`--panel-inset`, `--line` border, no relief — because at that alpha the fill
lands 1.07:1 against the panel and the key stops having a shape at all.

When you want to know what a control looks like, read the faceplate first.
When you want to *change* how a control looks, change the faceplate — a new
declaration in `styles.css` will not reach the screen.

## Reproducing the theme outside the app

Anything that renders RackForge markup — a test harness, a screenshot page,
a plugin surface — must load **all three** stylesheets in the order above.
Two of them are not the theme: `styles.css` on its own renders a cyan button
that exists nowhere in the product.

## Light and dark

Lighting is the player's choice of DAYLIGHT or STAGE, stamped on the
document element by `src/lighting.ts`:

| stamp | condition |
| --- | --- |
| *(none)* | follow `prefers-color-scheme` |
| `data-theme="light"` | DAYLIGHT, whatever the system says |
| `data-theme="dark"` | STAGE, whatever the system says |

Supporting "follow the system" without JavaScript means the dark palette is
written twice — once inside `@media (prefers-color-scheme: dark)` under
`:root:not([data-theme="light"])`, once under `:root[data-theme="dark"]` —
and the two copies must stay identical. `styles.css` carries 47 tokens in
each copy; `faceplate.css` has eight more paired blocks. **Changing a dark
value means changing it in both places**, and `src/theme.test.ts` checks
that the two copies still agree.
