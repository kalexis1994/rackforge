/**
 * `<rf-program-select>`: the program selector RackForge's own instruments
 * put in their headers. One implementation, served by RackForge and
 * injected into every plugin frame; each plugin places it where it likes and
 * styles it as its own (CSS custom properties, `::part()`, the arrow slots).
 *
 * It reads the programs from the host's context and chooses with
 * `plugin.select_sound`, so a plugin needs no code for it. A plugin that
 * wants its own flow sets `source="manual"`, gives it `programs`, `banks` and
 * `value`, and listens for `rf-program-select`.
 *
 * Documented in docs/WEB_PLUGIN_API.md, "Program selector".
 */
import { readContext, readResponse, readyMessage, selectRequest, type ProgramContext } from "./hostLink";
import { bankName, searchPrograms, stepProgram, usedBanks, type Bank, type Program } from "./programs";

/** More than this and the list asks for a narrower search: thousands of rows help no one. */
const MAX_ROWS = 300;

// One context shared by every selector in the frame, kept from the moment the
// module loads: an element placed later still knows the programs.
let latestContext: ProgramContext | null = null;
let readyAsked = false;
const selectors = new Set<RfProgramSelect>();
let requestCounter = 0;

function hostWindow(): Window | null {
  return window.parent && window.parent !== window ? window.parent : null;
}

window.addEventListener("message", (event) => {
  if (event.source !== hostWindow() || event.origin !== window.location.origin) return;
  const context = readContext(event.data);
  if (context) {
    latestContext = context;
    for (const selector of selectors) selector.hostContext(context);
    return;
  }
  for (const selector of selectors) selector.hostMessage(event.data);
});

const STYLE = /* css */ `
:host {
  --rf-ps-font: inherit;
  --rf-ps-color: currentColor;
  --rf-ps-muted: color-mix(in srgb, currentColor 60%, transparent);
  --rf-ps-accent: #6aa9ff;
  --rf-ps-background: transparent;
  --rf-ps-surface: #1c1f24;
  --rf-ps-surface-color: #e8ebef;
  --rf-ps-border: color-mix(in srgb, currentColor 25%, transparent);
  --rf-ps-radius: 6px;
  --rf-ps-height: 40px;
  --rf-ps-gap: 6px;
  --rf-ps-backdrop: rgb(0 0 0 / 55%);
  display: inline-flex;
  min-width: 0;
  max-width: 100%;
  font: var(--rf-ps-font);
  color: var(--rf-ps-color);
  /* Text styles inherit into the shadow tree, and a header's centring or
     capitals would reach the list; a plugin sets these on the parts. */
  text-align: start;
  text-transform: none;
  text-indent: 0;
  letter-spacing: normal;
  white-space: normal;
}
:host([hidden]) { display: none; }
.bar {
  display: flex;
  align-items: stretch;
  gap: var(--rf-ps-gap);
  min-width: 0;
  width: 100%;
  height: var(--rf-ps-height);
}
button {
  font: inherit;
  color: inherit;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}
button:focus-visible, input:focus-visible, [role="option"]:focus-visible {
  outline: 2px solid var(--rf-ps-accent);
  outline-offset: 1px;
}
.step {
  flex: none;
  min-width: var(--rf-ps-height);
  padding: 0;
  display: grid;
  place-items: center;
  background: var(--rf-ps-background);
  border: 1px solid var(--rf-ps-border);
  border-radius: var(--rf-ps-radius);
  font-size: 1.25em;
  line-height: 1;
}
:host([arrows="none"]) .step { display: none; }
.name {
  flex: 1 1 auto;
  min-width: 0;
  padding: 0 0.75em;
  display: flex;
  align-items: center;
  gap: 0.6em;
  background: var(--rf-ps-background);
  border: 1px solid var(--rf-ps-border);
  border-radius: var(--rf-ps-radius);
  text-align: start;
}
.number { flex: none; color: var(--rf-ps-muted); font-variant-numeric: tabular-nums; }
.text { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
.bank { flex: none; max-width: 40%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--rf-ps-muted); }
:host([hide-number]) .number, :host([hide-bank]) .bank, .number:empty, .bank:empty { display: none; }
/* The name comes first: on a narrow bar the bank goes, then the number.
   Hidden overflow keeps a sliding name inside the field. */
.name { container-type: inline-size; overflow: hidden; }
@container (max-width: 300px) { .bank { display: none; } }
@container (max-width: 190px) { .number { display: none; } }
.step:disabled, .name:disabled { cursor: default; opacity: 0.5; }

dialog {
  width: min(520px, 100vw - 24px);
  max-height: min(80vh, 100dvh - 24px);
  padding: 0;
  color: var(--rf-ps-surface-color);
  background: var(--rf-ps-surface);
  border: 1px solid var(--rf-ps-border);
  border-radius: calc(var(--rf-ps-radius) * 2);
  font: var(--rf-ps-font);
  overflow: hidden;
}
dialog[open] { display: flex; flex-direction: column; }
dialog::backdrop { background: var(--rf-ps-backdrop); }
@media (max-width: 560px), (max-height: 520px) {
  dialog { width: 100vw; max-width: 100vw; height: 100dvh; max-height: 100dvh; border-radius: 0; border: 0; }
}
/* Only the list scrolls: the search, the banks and the note keep their size. */
.header, .banks, .note { flex: none; }
.header { display: flex; gap: 8px; padding: 10px; }
.search {
  flex: 1 1 auto;
  min-width: 0;
  height: 44px;
  padding: 0 12px;
  color: inherit;
  background: color-mix(in srgb, var(--rf-ps-surface-color) 8%, transparent);
  border: 1px solid var(--rf-ps-border);
  border-radius: var(--rf-ps-radius);
  font: inherit;
  font-size: 16px;
}
.close {
  flex: none;
  width: 44px;
  height: 44px;
  background: transparent;
  border: 1px solid var(--rf-ps-border);
  border-radius: var(--rf-ps-radius);
  font-size: 1.3em;
}
.banks { display: flex; gap: 6px; padding: 0 10px 8px; overflow-x: auto; scrollbar-width: thin; }
.banks:empty { display: none; }
.chip {
  flex: none;
  height: 34px;
  padding: 0 12px;
  background: transparent;
  border: 1px solid var(--rf-ps-border);
  border-radius: 999px;
  white-space: nowrap;
}
.chip[aria-pressed="true"] { color: var(--rf-ps-surface); background: var(--rf-ps-accent); border-color: var(--rf-ps-accent); }
.list { flex: 1 1 auto; margin: 0; padding: 4px 6px 10px; overflow-y: auto; list-style: none; overscroll-behavior: contain; }
.item {
  min-height: 44px;
  padding: 6px 10px;
  display: grid;
  /* One width for every number, so the names line up from 9 to 121. */
  grid-template-columns: 3.5ch 1fr;
  grid-template-areas: "number name" "number detail";
  align-items: center;
  column-gap: 12px;
  border-radius: var(--rf-ps-radius);
  cursor: pointer;
}
.item[data-active] { background: color-mix(in srgb, var(--rf-ps-accent) 18%, transparent); }
.item[aria-selected="true"] { box-shadow: inset 3px 0 0 var(--rf-ps-accent); }
.item[aria-selected="true"] .item-name { color: var(--rf-ps-accent); }
.item-number { grid-area: number; color: var(--rf-ps-muted); font-variant-numeric: tabular-nums; text-align: end; }
.item-name { grid-area: name; font-weight: 600; overflow-wrap: anywhere; }
.item-detail { grid-area: detail; color: var(--rf-ps-muted); font-size: 0.85em; }
.item-detail:empty { display: none; }
.note { margin: 0; padding: 12px 16px; color: var(--rf-ps-muted); text-align: center; }
.note:empty { display: none; }
`;

export class RfProgramSelect extends HTMLElement {
  static get observedAttributes() {
    return ["placeholder", "label", "disabled", "fit"];
  }

  #programs: Program[] = [];
  #banks: Bank[] = [];
  #value: string | null = null;
  #query = "";
  #bank: string | null = null;
  #active = 0;
  #rows: Program[] = [];
  #pending: { requestId: string; previous: string | null } | null = null;
  #shownIndex = -1;
  #resize: ResizeObserver | null = null;
  #moved: { searching: boolean; scroll: number } | null = null;
  readonly #root: ShadowRoot;
  readonly #els: {
    prev: HTMLButtonElement;
    next: HTMLButtonElement;
    name: HTMLButtonElement;
    number: HTMLElement;
    text: HTMLElement;
    fit: HTMLElement;
    bank: HTMLElement;
    dialog: HTMLDialogElement;
    search: HTMLInputElement;
    close: HTMLButtonElement;
    banks: HTMLElement;
    list: HTMLUListElement;
    note: HTMLElement;
  };

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: "open" });
    this.#root.innerHTML = `
      <style>${STYLE}</style>
      <div class="bar" part="bar">
        <button class="step" part="prev step" type="button" aria-label="Previous program"><slot name="prev">‹</slot></button>
        <button class="name" part="name" type="button" aria-haspopup="dialog">
          <span class="number" part="number"></span>
          <span class="text" part="name-text"><span class="fit"></span></span>
          <span class="bank" part="bank"></span>
        </button>
        <button class="step" part="next step" type="button" aria-label="Next program"><slot name="next">›</slot></button>
      </div>
      <dialog part="dialog" aria-label="Programs">
        <div class="header" part="header">
          <input class="search" part="search" type="search" autocomplete="off" autocorrect="off"
            autocapitalize="off" spellcheck="false" enterkeyhint="go" role="combobox"
            aria-autocomplete="list" aria-expanded="true" aria-controls="list">
          <button class="close" part="close" type="button" aria-label="Close">×</button>
        </div>
        <div class="banks" part="banks" role="group" aria-label="Banks"></div>
        <ul class="list" part="list" id="list" role="listbox" aria-label="Programs"></ul>
        <p class="note" part="note" role="status"></p>
      </dialog>`;
    const find = <T extends Element>(selector: string) => this.#root.querySelector(selector) as T;
    this.#els = {
      prev: find(".step[aria-label='Previous program']"),
      next: find(".step[aria-label='Next program']"),
      name: find(".name"),
      number: find(".number"),
      text: find(".text"),
      fit: find(".fit"),
      bank: find(".bank"),
      dialog: find("dialog"),
      search: find(".search"),
      close: find(".close"),
      banks: find(".banks"),
      list: find(".list"),
      note: find(".note"),
    };
    const els = this.#els;
    els.prev.addEventListener("click", () => this.step(-1));
    els.next.addEventListener("click", () => this.step(1));
    els.name.addEventListener("click", () => this.open());
    els.close.addEventListener("click", () => this.close());
    els.dialog.addEventListener("close", () => this.#closed());
    // A tap on the backdrop is outside the panel's box.
    els.dialog.addEventListener("click", (event) => {
      if (event.target === els.dialog) {
        const box = els.dialog.getBoundingClientRect();
        const inside =
          event.clientX >= box.left && event.clientX <= box.right &&
          event.clientY >= box.top && event.clientY <= box.bottom;
        if (!inside) this.close();
      }
    });
    els.search.addEventListener("input", () => {
      this.#query = els.search.value;
      this.#active = 0;
      this.#renderList();
    });
    els.dialog.addEventListener("keydown", (event) => this.#key(event));
    els.list.addEventListener("click", (event) => {
      const item = (event.target as Element).closest<HTMLElement>("[data-id]");
      if (item?.dataset.id) this.#choose(item.dataset.id, true);
    });
    els.banks.addEventListener("click", (event) => {
      const chip = (event.target as Element).closest<HTMLElement>("[data-bank]");
      if (!chip) return;
      const bank = chip.dataset.bank ?? "";
      this.#bank = bank === "" ? null : this.#bank === bank ? null : bank;
      this.#active = 0;
      this.#renderBanks();
      this.#renderList();
    });
  }

  connectedCallback() {
    // RackForge loads this module after the plugin's page, so a plugin can
    // have set `programs` or `value` on the plain element first. Those are
    // own properties hiding the accessors; hand them over.
    const own = this as unknown as Record<string, unknown>;
    for (const property of ["programs", "banks", "value", "disabled"]) {
      if (Object.prototype.hasOwnProperty.call(own, property)) {
        const value = own[property];
        delete own[property];
        own[property] = value;
      }
    }
    selectors.add(this);
    if (!this.#isManual()) {
      if (latestContext) this.hostContext(latestContext);
      else if (!readyAsked && hostWindow()) {
        readyAsked = true;
        hostWindow()?.postMessage(readyMessage(), window.location.origin);
      }
    }
    this.#render();
    // The field a name has to fit changes with the page, not only with the
    // name: a host resizing the frame, a panel folding away, a font arriving.
    if (typeof ResizeObserver === "function") {
      let frame = 0;
      this.#resize = new ResizeObserver(() => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(() => this.#fit());
      });
      this.#resize.observe(this.#els.name);
    }
    void document.fonts?.ready.then(() => this.#fit());
    this.#restoreAfterMove();
  }

  disconnectedCallback() {
    selectors.delete(this);
    this.#resize?.disconnect();
    this.#resize = null;
    // A panel that redraws its page moves this element out and back in the
    // same task. Leaving the document takes an open panel out of the top
    // layer, so remember it and put it back on the way in.
    const { dialog, search, list } = this.#els;
    this.#moved = dialog.open
      ? { searching: this.#root.activeElement === search, scroll: list.scrollTop }
      : null;
  }

  #restoreAfterMove() {
    const moved = this.#moved;
    this.#moved = null;
    const dialog = this.#els.dialog;
    if (!moved || !dialog.open) return;
    try {
      if (typeof dialog.showModal === "function" && !dialog.matches(":modal")) {
        // Removing the attribute rather than calling close() keeps the
        // `close` event, and the plugin's rf-program-close, from firing.
        dialog.removeAttribute("open");
        dialog.showModal();
      }
    } catch {
      // An engine without :modal keeps the panel open, if not modal.
    }
    this.#els.list.scrollTop = moved.scroll;
    if (moved.searching) this.#els.search.focus({ preventScroll: true });
    else this.#activeItem()?.focus({ preventScroll: true });
  }

  attributeChangedCallback() {
    this.#render();
    this.#fit();
  }

  /** The programs, in the plugin's order. */
  get programs(): Program[] {
    return this.#programs;
  }
  set programs(programs: Program[]) {
    this.#programs = Array.isArray(programs) ? programs : [];
    this.#render();
  }

  get banks(): Bank[] {
    return this.#banks;
  }
  set banks(banks: Bank[]) {
    this.#banks = Array.isArray(banks) ? banks : [];
    this.#render();
  }

  /** The chosen program's id. */
  get value(): string | null {
    return this.#value;
  }
  set value(value: string | null) {
    this.#value = value;
    this.#render();
  }

  /** A plugin in the middle of something (saving a program, say) turns it off. */
  get disabled(): boolean {
    return this.hasAttribute("disabled");
  }
  set disabled(disabled: boolean) {
    this.toggleAttribute("disabled", Boolean(disabled));
  }

  /** Chooses the program `delta` places away, wrapping round. */
  step(delta: number) {
    if (this.disabled) return;
    const id = stepProgram(this.#programs, this.#value, delta);
    if (id) this.#choose(id, false);
  }

  open() {
    if (this.disabled || this.#programs.length === 0 || this.#els.dialog.open) return;
    this.#query = "";
    this.#els.search.value = "";
    this.#bank = null;
    const at = this.#programs.findIndex((program) => program.id === this.#value);
    this.#active = Math.max(0, at);
    this.#renderBanks();
    this.#renderList();
    const dialog = this.#els.dialog;
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
    // A keyboard at hand gets the search; a touch screen does not have its
    // own keyboard thrown up over the list.
    if (window.matchMedia?.("(pointer: fine)").matches) this.#els.search.focus();
    else this.#activeItem()?.focus();
    this.#activeItem()?.scrollIntoView({ block: "center" });
    this.dispatchEvent(new CustomEvent("rf-program-open", { bubbles: true, composed: true }));
  }

  close() {
    const dialog = this.#els.dialog;
    if (!dialog.open) return;
    if (typeof dialog.close === "function") dialog.close();
    else {
      dialog.removeAttribute("open");
      this.#closed();
    }
  }

  /** @internal A context from the host. */
  hostContext(context: ProgramContext) {
    if (this.#isManual()) return;
    this.#programs = context.programs;
    this.#banks = context.banks;
    // A choice on its way holds the name until the host has it: a context
    // sent meanwhile, for a parameter, still names the old program.
    if (this.#pending && context.selected !== this.#value) {
      this.#render();
      if (this.#els.dialog.open) this.#renderList();
      return;
    }
    this.#pending = null;
    this.#value = context.selected;
    this.#render();
    if (this.#els.dialog.open) this.#renderList();
  }

  /** @internal Any other message from the host. */
  hostMessage(message: unknown) {
    if (!this.#pending) return;
    const response = readResponse(message, this.#pending.requestId);
    if (!response) return;
    const { previous } = this.#pending;
    this.#pending = null;
    if (!response.ok) {
      this.#value = previous;
      this.#render();
      this.dispatchEvent(
        new CustomEvent("rf-program-error", {
          bubbles: true,
          composed: true,
          detail: { error: response.error ?? "The program could not be chosen." },
        }),
      );
    }
  }

  #isManual() {
    return this.getAttribute("source") === "manual";
  }

  #choose(id: string, fromList: boolean) {
    const program = this.#programs.find((candidate) => candidate.id === id);
    if (!program || this.disabled) return;
    const event = new CustomEvent("rf-program-select", {
      bubbles: true,
      composed: true,
      cancelable: true,
      detail: { id, program },
    });
    const proceed = this.dispatchEvent(event);
    if (fromList && !this.hasAttribute("stay-open")) this.close();
    if (!proceed || id === this.#value) return;
    const previous = this.#value;
    this.#value = id;
    this.#render();
    if (fromList && this.#els.dialog.open) this.#renderList();
    if (this.#isManual()) return;
    const host = hostWindow();
    if (!host) return;
    requestCounter += 1;
    const requestId = `rf-program-select-${requestCounter}`;
    this.#pending = { requestId, previous };
    host.postMessage(selectRequest(requestId, id), window.location.origin);
  }

  #closed() {
    this.#els.name.focus({ preventScroll: true });
    this.dispatchEvent(new CustomEvent("rf-program-close", { bubbles: true, composed: true }));
  }

  #key(event: KeyboardEvent) {
    const rows = this.#rows;
    const move = (to: number) => {
      if (rows.length === 0) return;
      this.#active = Math.max(0, Math.min(rows.length - 1, to));
      this.#markActive();
      event.preventDefault();
    };
    switch (event.key) {
      case "ArrowDown":
        move(this.#active + 1);
        break;
      case "ArrowUp":
        move(this.#active - 1);
        break;
      case "PageDown":
        move(this.#active + 8);
        break;
      case "PageUp":
        move(this.#active - 8);
        break;
      case "Home":
        if (event.target !== this.#els.search) move(0);
        break;
      case "End":
        if (event.target !== this.#els.search) move(rows.length - 1);
        break;
      case "Enter": {
        const row = rows[this.#active];
        if (row) {
          event.preventDefault();
          this.#choose(row.id, true);
        }
        break;
      }
      default:
        // Typing anywhere in the panel searches.
        if (
          event.key.length === 1 &&
          !event.ctrlKey && !event.metaKey && !event.altKey &&
          event.target !== this.#els.search
        ) {
          this.#els.search.focus();
        }
    }
  }

  #render() {
    const els = this.#els;
    const index = this.#programs.findIndex((program) => program.id === this.#value);
    const program = index >= 0 ? this.#programs[index] : undefined;
    const label = this.getAttribute("label") ?? "Program";
    const shown =
      program?.name ?? (this.#programs.length === 0 ? this.getAttribute("empty-label") ?? "No programs" : "—");
    const previous = this.#shownIndex;
    this.#shownIndex = index;
    if (els.fit.textContent !== shown) {
      els.fit.textContent = shown;
      if (this.hasAttribute("slide") && previous >= 0 && index >= 0 && previous !== index) {
        this.#slide(Math.sign(index - previous));
      }
      this.#fit();
    }
    els.number.textContent = index >= 0 ? `${index + 1}/${this.#programs.length}` : "";
    els.bank.textContent =
      usedBanks(this.#programs, this.#banks).length > 1 ? (bankName(this.#banks, program?.bank) ?? "") : "";
    els.name.setAttribute(
      "aria-label",
      program ? `${label}: ${program.name}. Choose another` : `${label}: none`,
    );
    const off = this.disabled;
    els.name.disabled = off || this.#programs.length === 0;
    els.prev.disabled = off || this.#programs.length < 2;
    els.next.disabled = off || this.#programs.length < 2;
    if (off) this.close();
    els.search.placeholder = this.getAttribute("placeholder") ?? "Search programs";
    els.search.setAttribute("aria-label", this.getAttribute("placeholder") ?? "Search programs");
  }

  /**
   * `fit`: a long name is lettered smaller, down to the given share of its
   * size (0.62 by default), before it is cut with an ellipsis -- what tells
   * two programs apart tends to sit at the end of their names. The inner span
   * scales in `em`, so the size a plugin gives `::part(name-text)` stays the
   * one a name gets when it fits.
   */
  #fit() {
    const { text, fit } = this.#els;
    fit.style.fontSize = "";
    if (!this.hasAttribute("fit") || !this.isConnected) return;
    const floor = Math.min(1, Math.max(0.3, Number(this.getAttribute("fit")) || 0.62));
    for (let scale = 1; scale > floor && text.scrollWidth > text.clientWidth; ) {
      scale = Math.max(floor, scale - 0.02);
      fit.style.fontSize = `${scale}em`;
    }
  }

  /** `slide`: the name arrives from the side the choice moved towards. */
  #slide(direction: number) {
    const text = this.#els.text;
    if (typeof text.animate !== "function") return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    const animation = text.animate(
      [
        { transform: `translateX(${direction * 0.9}em)`, opacity: 0 },
        { transform: "none", opacity: 1 },
      ],
      { duration: 160, easing: "ease-out" },
    );
    // A hidden frame can have a frozen timeline, where the animation would
    // hold its first frame and leave the name invisible. Cancelling puts it
    // at rest whether it ran or not.
    window.setTimeout(() => animation.cancel(), 400);
  }

  #renderBanks() {
    const banks = usedBanks(this.#programs, this.#banks);
    const box = this.#els.banks;
    box.replaceChildren();
    if (banks.length < 2) return;
    const chip = (id: string, name: string, pressed: boolean) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "chip";
      button.setAttribute("part", pressed ? "bank-chip bank-chip-selected" : "bank-chip");
      button.dataset.bank = id;
      button.setAttribute("aria-pressed", String(pressed));
      button.textContent = name;
      return button;
    };
    box.append(chip("", this.getAttribute("all-label") ?? "All", this.#bank === null));
    for (const bank of banks) box.append(chip(bank.id, bank.name, this.#bank === bank.id));
  }

  #renderList() {
    const matches = searchPrograms(this.#programs, this.#banks, this.#query, this.#bank);
    const shown = matches.slice(0, MAX_ROWS);
    // A row names its bank only when there is more than one to tell apart
    // and the list is not already showing a single one.
    const nameBanks = this.#bank === null && usedBanks(this.#programs, this.#banks).length > 1;
    this.#rows = shown.map((match) => match.program);
    this.#active = Math.min(this.#active, Math.max(0, this.#rows.length - 1));
    const fragment = document.createDocumentFragment();
    for (const match of shown) {
      const item = document.createElement("li");
      item.className = "item";
      item.id = `option-${match.index}`;
      item.dataset.id = match.program.id;
      item.setAttribute("role", "option");
      item.tabIndex = -1;
      const selected = match.program.id === this.#value;
      item.setAttribute("aria-selected", String(selected));
      item.setAttribute("part", selected ? "item item-selected" : "item");
      const number = document.createElement("span");
      number.className = "item-number";
      number.setAttribute("part", "item-number");
      number.textContent = String(match.index + 1);
      const name = document.createElement("span");
      name.className = "item-name";
      name.setAttribute("part", "item-name");
      name.textContent = match.program.name;
      const detail = document.createElement("span");
      detail.className = "item-detail";
      detail.setAttribute("part", "item-detail");
      detail.textContent = [
        nameBanks ? bankName(this.#banks, match.program.bank) : undefined,
        match.program.detail,
      ]
        .filter(Boolean)
        .join(" · ");
      item.append(number, name, detail);
      fragment.append(item);
    }
    this.#els.list.replaceChildren(fragment);
    const hidden = matches.length - shown.length;
    this.#els.note.textContent =
      matches.length === 0
        ? this.getAttribute("no-match-label") ?? "No program matches"
        : hidden > 0
          ? `${hidden} more: keep typing to narrow`
          : "";
    this.#markActive();
  }

  #activeItem(): HTMLElement | null {
    return this.#els.list.children[this.#active] as HTMLElement | null;
  }

  #markActive() {
    for (const item of this.#els.list.children) (item as HTMLElement).removeAttribute("data-active");
    const item = this.#activeItem();
    if (!item) {
      this.#els.search.removeAttribute("aria-activedescendant");
      return;
    }
    item.setAttribute("data-active", "");
    this.#els.search.setAttribute("aria-activedescendant", item.id);
    item.scrollIntoView({ block: "nearest" });
    if (this.#root.activeElement !== this.#els.search) item.focus({ preventScroll: true });
  }
}

if (!customElements.get("rf-program-select")) {
  customElements.define("rf-program-select", RfProgramSelect);
}

declare global {
  interface HTMLElementTagNameMap {
    "rf-program-select": RfProgramSelect;
  }
}
