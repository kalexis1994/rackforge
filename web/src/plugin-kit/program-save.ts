/**
 * `<rf-program-save>`: the dialog RackForge's instruments use to save what is
 * playing as a program. One implementation, served by RackForge and injected
 * into every plugin frame beside `<rf-program-select>`; each plugin opens it
 * from its own control (a RECORD key, a menu) and styles it as its own: CSS
 * custom properties, `::part()`, slots for every label and button, and
 * attributes for every text.
 *
 * It saves with the host's program editing methods: a draft of a new program
 * from what is playing, or of the current one when it is the plugin's own;
 * the name; the save. A plugin that wants its own path cancels
 * `rf-program-save` (or sets `source="manual"`) and calls `done()` or
 * `fail()` when it has finished.
 *
 * Documented in docs/WEB_PLUGIN_API.md, "Program save dialog".
 */
import {
  beginProgramEditRequest,
  cancelProgramRequest,
  programNameRequest,
  readContext,
  readProgramDraft,
  readResponse,
  readyMessage,
  saveProgramRequest,
  type ProgramContext,
  type ProgramDraft,
} from "./hostLink";
import {
  MAX_NAME_LENGTH,
  nameProblem,
  replaceable,
  saveModes,
  startingName,
  type SaveMode,
} from "./program-save-logic";

/** How long the host has to answer a request, or to send the draft it began. */
const HOST_TIMEOUT_MS = 8000;

// One context shared by every dialog in the frame, kept from the moment the
// module loads: a dialog placed later still knows the programs and the draft.
let latestContext: ProgramContext | null = null;
let latestDraft: ProgramDraft | null = null;
let readyAsked = false;
const dialogs = new Set<RfProgramSave>();
let requestCounter = 0;

function hostWindow(): Window | null {
  return window.parent && window.parent !== window ? window.parent : null;
}

function nextRequestId(): string {
  requestCounter += 1;
  return `rf-program-save-${requestCounter}`;
}

window.addEventListener("message", (event) => {
  if (event.source !== hostWindow() || event.origin !== window.location.origin) return;
  const context = readContext(event.data);
  if (context) {
    latestContext = context;
    latestDraft = readProgramDraft(event.data) ?? null;
    for (const dialog of dialogs) dialog.hostContext();
    return;
  }
  for (const dialog of dialogs) dialog.hostMessage(event.data);
});

/** The texts a plugin can change with attributes, and what they say otherwise. */
const TEXTS = {
  // Not `title`, which a browser would show as a tooltip over the dialog.
  heading: "Save program",
  label: "Name",
  placeholder: "Program name",
  "cancel-label": "Cancel",
  "replace-label": "Replace",
  "save-label": "Save as new",
  "current-label": "Current program:",
  "busy-label": "Saving…",
  "default-name": "New program",
  "empty-error": "Name the program first.",
  "long-error": `Keep the name to ${MAX_NAME_LENGTH} characters.`,
  "control-error": "The name has characters that cannot be saved.",
  "error-label": "The program could not be saved:",
} as const;
type TextName = keyof typeof TEXTS;

const STYLE = /* css */ `
:host {
  --rf-save-font: inherit;
  --rf-save-color: #e8ebef;
  --rf-save-muted: color-mix(in srgb, var(--rf-save-color) 60%, transparent);
  --rf-save-accent: #6aa9ff;
  --rf-save-danger: #ff7a70;
  --rf-save-surface: #1c1f24;
  --rf-save-border: color-mix(in srgb, var(--rf-save-color) 25%, transparent);
  --rf-save-radius: 8px;
  --rf-save-backdrop: rgb(0 0 0 / 55%);
  --rf-save-width: 420px;
  --rf-save-padding: 18px;
  --rf-save-gap: 12px;
  --rf-save-field-height: 44px;
  --rf-save-field-font: inherit;
  --rf-save-field-color: inherit;
  --rf-save-field-background: color-mix(in srgb, var(--rf-save-color) 8%, transparent);
  --rf-save-button-height: 40px;
  --rf-save-button-background: transparent;
  --rf-save-primary-background: var(--rf-save-accent);
  --rf-save-primary-color: var(--rf-save-surface);
  display: contents;
  font: var(--rf-save-font);
  /* Text styles inherit into the shadow tree; a plugin sets these on the parts. */
  text-align: start;
  text-transform: none;
  letter-spacing: normal;
}
:host([hidden]) { display: none; }
dialog {
  box-sizing: border-box;
  width: min(var(--rf-save-width), 100vw - 24px);
  max-height: min(90vh, 100dvh - 24px);
  padding: 0;
  color: var(--rf-save-color);
  background: var(--rf-save-surface);
  border: 1px solid var(--rf-save-border);
  border-radius: var(--rf-save-radius);
  font: var(--rf-save-font);
  overflow: auto;
}
dialog::backdrop { background: var(--rf-save-backdrop); }
.form {
  margin: 0;
  padding: var(--rf-save-padding);
  display: flex;
  flex-direction: column;
  gap: var(--rf-save-gap);
}
.header { display: flex; align-items: center; gap: var(--rf-save-gap); }
.title { margin: 0; font-size: 1.1em; font-weight: 700; }
.body { display: flex; flex-direction: column; gap: 6px; }
.label { color: var(--rf-save-muted); font-size: 0.85em; }
.name {
  box-sizing: border-box;
  width: 100%;
  height: var(--rf-save-field-height);
  padding: 0 12px;
  color: var(--rf-save-field-color);
  background: var(--rf-save-field-background);
  border: 1px solid var(--rf-save-border);
  border-radius: calc(var(--rf-save-radius) * 0.75);
  font: var(--rf-save-field-font);
  /* A phone does not zoom into a field at 16px and above. */
  font-size: max(16px, 1em);
}
.current, .error, .status { margin: 0; font-size: 0.85em; }
.current { color: var(--rf-save-muted); }
.error { color: var(--rf-save-danger); }
.current:empty, .error:empty, .status:empty { display: none; }
.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
.action {
  min-height: var(--rf-save-button-height);
  padding: 0 14px;
  color: inherit;
  background: var(--rf-save-button-background);
  border: 1px solid var(--rf-save-border);
  border-radius: calc(var(--rf-save-radius) * 0.75);
  font: inherit;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}
.action[data-primary] {
  color: var(--rf-save-primary-color);
  background: var(--rf-save-primary-background);
  border-color: var(--rf-save-primary-background);
}
.action[hidden] { display: none; }
.action:disabled { cursor: default; opacity: 0.5; }
.name:focus-visible, .action:focus-visible {
  outline: 2px solid var(--rf-save-accent);
  outline-offset: 1px;
}
:host([hide-cancel]) .cancel, :host([hide-title]) .header { display: none; }
@media (max-width: 560px) {
  dialog { width: calc(100vw - 16px); }
}
`;

export interface ProgramSaveDetail {
  name: string;
  mode: SaveMode;
  /** The program saved over; null for a new one. */
  programId: string | null;
}

export class RfProgramSave extends HTMLElement {
  static get observedAttributes() {
    return [...Object.keys(TEXTS), "primary", "disabled"];
  }

  #busy = false;
  #waiting: { requestId: string; settle: (error: string | null) => void } | null = null;
  #draftWaiters: Array<() => void> = [];
  #pendingSave: ProgramSaveDetail | null = null;
  #wasOpen = false;
  readonly #root: ShadowRoot;
  readonly #els: {
    dialog: HTMLDialogElement;
    form: HTMLFormElement;
    title: HTMLElement;
    label: HTMLElement;
    name: HTMLInputElement;
    current: HTMLElement;
    error: HTMLElement;
    status: HTMLElement;
    cancel: HTMLButtonElement;
    cancelText: HTMLElement;
    replace: HTMLButtonElement;
    replaceText: HTMLElement;
    save: HTMLButtonElement;
    saveText: HTMLElement;
  };

  constructor() {
    super();
    this.#root = this.attachShadow({ mode: "open" });
    this.#root.innerHTML = `
      <style>${STYLE}</style>
      <dialog part="dialog" aria-labelledby="title">
        <form class="form" part="form" novalidate>
          <div class="header" part="header">
            <slot name="header-start"></slot>
            <h2 class="title" part="title" id="title"><slot name="title"><span data-text="heading"></span></slot></h2>
            <slot name="header-end"></slot>
          </div>
          <div class="body" part="body">
            <slot name="body-start"></slot>
            <label class="label" part="label" for="name"><slot name="label"><span data-text="label"></span></slot></label>
            <input class="name" part="name" id="name" type="text" maxlength="${MAX_NAME_LENGTH}"
              autocomplete="off" autocorrect="off" spellcheck="false" enterkeyhint="done">
            <p class="current" part="current"></p>
            <p class="error" part="error" role="alert"></p>
            <slot name="body-end"></slot>
          </div>
          <div class="actions" part="actions">
            <button class="action cancel" part="action cancel" type="button"><slot name="cancel"><span data-text="cancel-label"></span></slot></button>
            <button class="action replace" part="action replace" type="button"><slot name="replace"><span data-text="replace-label"></span></slot></button>
            <button class="action save" part="action save" type="button"><slot name="save"><span data-text="save-label"></span></slot></button>
          </div>
          <p class="status" part="status" role="status"></p>
          <slot name="footer"></slot>
        </form>
      </dialog>`;
    const find = <T extends Element>(selector: string) => this.#root.querySelector(selector) as T;
    this.#els = {
      dialog: find("dialog"),
      form: find(".form"),
      title: find("[data-text='heading']"),
      label: find("[data-text='label']"),
      name: find(".name"),
      current: find(".current"),
      error: find(".error"),
      status: find(".status"),
      cancel: find(".cancel"),
      cancelText: find("[data-text='cancel-label']"),
      replace: find(".replace"),
      replaceText: find("[data-text='replace-label']"),
      save: find(".save"),
      saveText: find("[data-text='save-label']"),
    };
    const els = this.#els;
    els.cancel.addEventListener("click", () => this.close());
    els.replace.addEventListener("click", () => void this.save("replace"));
    els.save.addEventListener("click", () => void this.save("new"));
    els.form.addEventListener("submit", (event) => {
      event.preventDefault();
      void this.save();
    });
    els.name.addEventListener("input", () => {
      if (!this.#busy) els.error.textContent = "";
    });
    // Escape: nothing is left half saved.
    els.dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      if (!this.#busy) this.close();
    });
    els.dialog.addEventListener("close", () => this.#closed());
    // A tap on the backdrop is outside the dialog's box.
    els.dialog.addEventListener("click", (event) => {
      if (event.target !== els.dialog || this.#busy) return;
      const box = els.dialog.getBoundingClientRect();
      const inside =
        event.clientX >= box.left && event.clientX <= box.right &&
        event.clientY >= box.top && event.clientY <= box.bottom;
      if (!inside) this.close();
    });
  }

  connectedCallback() {
    dialogs.add(this);
    if (!this.#isManual() && !latestContext && !readyAsked && hostWindow()) {
      readyAsked = true;
      hostWindow()?.postMessage(readyMessage(), window.location.origin);
    }
    this.#render();
    // A panel that redraws its page moves this element out and back in the
    // same task; leaving the document takes an open dialog out of the top layer.
    if (this.#wasOpen && this.#els.dialog.open) {
      this.#els.dialog.removeAttribute("open");
      this.#show();
    }
    this.#wasOpen = false;
  }

  disconnectedCallback() {
    dialogs.delete(this);
    this.#wasOpen = this.#els.dialog.open;
  }

  attributeChangedCallback() {
    this.#render();
  }

  /** The name in the field. */
  get name(): string {
    return this.#els.name.value;
  }
  set name(name: string) {
    this.#els.name.value = String(name ?? "").slice(0, MAX_NAME_LENGTH);
  }

  /** Whether the dialog is open. */
  get isOpen(): boolean {
    return this.#els.dialog.open;
  }

  /** Whether a save is on its way. */
  get busy(): boolean {
    return this.#busy;
  }

  /** Whether the current program is the plugin's own, and so can be saved over. */
  get canReplace(): boolean {
    return this.#current() !== null;
  }

  /**
   * Opens the dialog. `name` starts the field (else the current program's
   * name, else `default-name`); `mode` chooses what Enter does.
   */
  open(options: { name?: string; mode?: SaveMode } = {}) {
    if (this.hasAttribute("disabled") || this.#els.dialog.open) return;
    const current = this.#currentProgram();
    this.#els.name.value = startingName(options.name, current, this.#text("default-name"));
    if (options.mode) this.setAttribute("primary", options.mode);
    this.#els.error.textContent = "";
    this.#els.status.textContent = "";
    this.#render();
    this.#show();
    this.#els.name.focus();
    this.#els.name.select();
    this.dispatchEvent(new CustomEvent("rf-program-save-open", { bubbles: true, composed: true }));
  }

  /** Closes the dialog, unless a save is on its way. */
  close() {
    if (this.#busy) return;
    this.#reason = "cancel";
    this.#hide();
  }

  /**
   * Saves under the name in the field: over the current program
   * (`"replace"`, only when it is the plugin's own) or as a new one.
   * Without a mode, as Enter would.
   */
  async save(mode?: SaveMode): Promise<boolean> {
    if (this.#busy || this.hasAttribute("disabled")) return false;
    const current = this.#current();
    const { modes, primary } = saveModes(current !== null, this.#preferred());
    const chosen = mode ?? primary;
    if (!modes.includes(chosen)) return false;
    const name = this.#els.name.value.trim();
    const problem = nameProblem(name);
    if (problem) {
      this.#els.error.textContent = this.#text(`${problem}-error`);
      this.#els.name.focus();
      return false;
    }
    const detail: ProgramSaveDetail = {
      name,
      mode: chosen,
      programId: chosen === "replace" && current ? current.id : null,
    };
    this.#setBusy(true);
    const proceed = this.dispatchEvent(
      new CustomEvent<ProgramSaveDetail>("rf-program-save", {
        bubbles: true,
        composed: true,
        cancelable: true,
        detail,
      }),
    );
    if (!proceed || this.#isManual()) {
      // The plugin saves by its own path and calls done() or fail().
      this.#pendingSave = detail;
      return true;
    }
    return this.#saveWithHost(detail);
  }

  /** A plugin that saved by its own path reports that it has. */
  done() {
    const detail = this.#pendingSave;
    this.#pendingSave = null;
    if (detail) this.#finish(detail);
  }

  /** A plugin that saved by its own path reports that it could not. */
  fail(error = "") {
    this.#pendingSave = null;
    this.#failed(error);
  }

  /** @internal A context from the host. */
  hostContext() {
    this.#render();
    const waiters = this.#draftWaiters;
    this.#draftWaiters = [];
    for (const wake of waiters) wake();
  }

  /** @internal Any other message from the host. */
  hostMessage(message: unknown) {
    const waiting = this.#waiting;
    if (!waiting) return;
    const response = readResponse(message, waiting.requestId);
    if (!response) return;
    this.#waiting = null;
    waiting.settle(response.ok ? null : response.error ?? "");
  }

  #reason: "cancel" | "saved" = "cancel";

  async #saveWithHost(detail: ProgramSaveDetail): Promise<boolean> {
    let draftId: number | null = null;
    try {
      const before = latestDraft?.draftId ?? null;
      await this.#ask((requestId) => beginProgramEditRequest(requestId, detail.programId));
      const draft = await this.#draftAfter(before);
      draftId = draft.draftId;
      await this.#ask((requestId) => programNameRequest(requestId, draft.draftId, detail.name));
      await this.#ask((requestId) => saveProgramRequest(requestId, draft.draftId));
      draftId = null;
      this.#finish(detail);
      return true;
    } catch (error) {
      if (draftId !== null) {
        hostWindow()?.postMessage(cancelProgramRequest(nextRequestId(), draftId), window.location.origin);
      }
      this.#failed(error instanceof Error ? error.message : String(error));
      return false;
    }
  }

  /** Sends a request and waits for its answer; an answer that is not ok throws. */
  #ask(build: (requestId: string) => unknown): Promise<void> {
    const host = hostWindow();
    if (!host) return Promise.reject(new Error("There is no host to save to."));
    return new Promise<void>((resolve, reject) => {
      const requestId = nextRequestId();
      const timer = window.setTimeout(() => {
        if (this.#waiting?.requestId === requestId) this.#waiting = null;
        reject(new Error("RackForge did not answer in time."));
      }, HOST_TIMEOUT_MS);
      this.#waiting = {
        requestId,
        settle: (error) => {
          window.clearTimeout(timer);
          if (error === null) resolve();
          else reject(new Error(error));
        },
      };
      host.postMessage(build(requestId), window.location.origin);
    });
  }

  /** The draft the host began: it arrives with a context, before or after its answer. */
  #draftAfter(before: number | null): Promise<ProgramDraft> {
    return new Promise<ProgramDraft>((resolve, reject) => {
      const fresh = () => (latestDraft && latestDraft.draftId !== before ? latestDraft : null);
      const now = fresh();
      if (now) {
        resolve(now);
        return;
      }
      const timer = window.setTimeout(() => {
        reject(new Error("RackForge did not start the program."));
      }, HOST_TIMEOUT_MS);
      const wake = () => {
        const draft = fresh();
        if (draft) {
          window.clearTimeout(timer);
          resolve(draft);
        } else {
          this.#draftWaiters.push(wake);
        }
      };
      this.#draftWaiters.push(wake);
    });
  }

  #finish(detail: ProgramSaveDetail) {
    this.#setBusy(false);
    this.#reason = "saved";
    this.#hide();
    this.dispatchEvent(
      new CustomEvent<ProgramSaveDetail>("rf-program-saved", { bubbles: true, composed: true, detail }),
    );
  }

  #failed(error: string) {
    this.#setBusy(false);
    const said = error.trim();
    this.#els.error.textContent = said ? `${this.#text("error-label")} ${said}` : this.#text("error-label");
    this.dispatchEvent(
      new CustomEvent("rf-program-save-error", {
        bubbles: true,
        composed: true,
        detail: { error: said },
      }),
    );
  }

  #setBusy(busy: boolean) {
    this.#busy = busy;
    this.toggleAttribute("busy", busy);
    this.#els.status.textContent = busy ? this.#text("busy-label") : "";
    this.#render();
  }

  #show() {
    const dialog = this.#els.dialog;
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
  }

  #hide() {
    const dialog = this.#els.dialog;
    if (!dialog.open) return;
    if (typeof dialog.close === "function") dialog.close();
    else {
      dialog.removeAttribute("open");
      this.#closed();
    }
  }

  #closed() {
    this.dispatchEvent(
      new CustomEvent("rf-program-save-close", {
        bubbles: true,
        composed: true,
        detail: { reason: this.#reason },
      }),
    );
    this.#reason = "cancel";
  }

  #isManual() {
    return this.getAttribute("source") === "manual";
  }

  #preferred(): SaveMode | null {
    const primary = this.getAttribute("primary");
    return primary === "new" || primary === "replace" ? primary : null;
  }

  #currentProgram() {
    const context = latestContext;
    return context?.programs.find((program) => program.id === context.selected);
  }

  #current() {
    if (this.#isManual()) {
      // A plugin that saves by its own path says whether replacing is offered.
      const program = this.#currentProgram();
      return this.hasAttribute("can-replace") && program ? program : null;
    }
    const context = latestContext;
    return context ? replaceable(context.programs, context.selected) : null;
  }

  #text(name: TextName): string {
    return this.getAttribute(name) ?? TEXTS[name];
  }

  #render() {
    const els = this.#els;
    els.title.textContent = this.#text("heading");
    els.label.textContent = this.#text("label");
    els.cancelText.textContent = this.#text("cancel-label");
    els.replaceText.textContent = this.#text("replace-label");
    els.saveText.textContent = this.#text("save-label");
    els.name.placeholder = this.#text("placeholder");
    const current = this.#currentProgram();
    els.current.textContent = current ? `${this.#text("current-label")} ${current.name}` : "";
    const replaceableNow = this.#current() !== null;
    if (!this.#isManual()) this.toggleAttribute("can-replace", replaceableNow);
    const { modes, primary } = saveModes(replaceableNow, this.#preferred());
    els.replace.hidden = !modes.includes("replace");
    els.replace.toggleAttribute("data-primary", primary === "replace");
    els.save.toggleAttribute("data-primary", primary === "new");
    els.replace.setAttribute("part", `action replace${primary === "replace" ? " primary" : ""}`);
    els.save.setAttribute("part", `action save${primary === "new" ? " primary" : ""}`);
    const off = this.#busy || this.hasAttribute("disabled");
    els.name.disabled = off;
    els.replace.disabled = off;
    els.save.disabled = off;
    els.cancel.disabled = this.#busy;
  }
}

if (!customElements.get("rf-program-save")) {
  customElements.define("rf-program-save", RfProgramSave);
}

declare global {
  interface HTMLElementTagNameMap {
    "rf-program-save": RfProgramSave;
  }
}
