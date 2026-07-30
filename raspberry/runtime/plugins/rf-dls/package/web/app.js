(() => {
  "use strict";

  const PROTOCOL = "rackforge.plugin.web@1";
  const surface = document.body.dataset.surface;
  const root = document.getElementById("plugin-root");
  const hostOrigin = window.location.origin;
  let context = null;
  let sequence = 0;
  let activeCollection = "dls";
  let searchQuery = "";
  let configSearchQuery = "";
  let activeDraftId = null;
  let editMode = false;
  let localProgramName = null;
  let discardPrompt = false;
  let bridgeError = "";
  const localFieldValues = new Map();
  const previewTimers = new Map();

  function node(tag, className, text) {
    const element = document.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined) element.textContent = text;
    return element;
  }

  function request(method, params) {
    sequence += 1;
    window.parent.postMessage(
      {
        protocol: PROTOCOL,
        kind: "request",
        request_id: `rf-dls-${sequence}`,
        method,
        params,
      },
      hostOrigin,
    );
  }

  function sameValue(left, right) {
    return JSON.stringify(left) === JSON.stringify(right);
  }

  function displayedFieldValue(field) {
    const local = localFieldValues.get(field.id);
    if (local && sameValue(local, field.value)) {
      localFieldValues.delete(field.id);
      return field.value;
    }
    return local ?? field.value;
  }

  function editField(draft, field, value, preview) {
    localFieldValues.set(field.id, value);
    request("plugin.edit_program_field", {
      draft_id: draft.draft_id,
      field_id: field.id,
      value,
      preview,
    });
  }

  function previewField(draft, field, value) {
    localFieldValues.set(field.id, value);
    const existing = previewTimers.get(field.id);
    if (existing) window.clearTimeout(existing);
    previewTimers.set(
      field.id,
      window.setTimeout(() => {
        previewTimers.delete(field.id);
        request("plugin.edit_program_field", {
          draft_id: draft.draft_id,
          field_id: field.id,
          value,
          preview: true,
        });
      }, 70),
    );
  }

  function commitField(draft, field, value) {
    const existing = previewTimers.get(field.id);
    if (existing) window.clearTimeout(existing);
    previewTimers.delete(field.id);
    editField(draft, field, value, false);
  }

  function selectedSound(instance) {
    return instance.sounds.find(
      (candidate) => candidate.id === instance.selected_sound_id,
    );
  }

  function collectionFor(sound) {
    return sound?.bank?.toLowerCase() === "custom" ? "custom" : "dls";
  }

  function pluginHeader(instance, label) {
    const header = node("header", "plugin-header");
    const copy = node("div");
    copy.append(
      node("span", "eyebrow", label),
      node("h1", "", instance.plugin_name),
      node(
        "p",
        "",
        `${instance.sounds.length} programs · DLS instrument engine`,
      ),
    );
    const badge = node("span", "api-badge", "WEB API 1");
    header.append(copy, badge);
    return header;
  }

  function renderPlay(instance) {
    root.replaceChildren();
    root.append(pluginHeader(instance, "PLAY SURFACE"));

    const current = selectedSound(instance);
    const collections = [
      {
        id: "dls",
        label: "DLS",
        sounds: instance.sounds.filter(
          (sound) => collectionFor(sound) === "dls",
        ),
      },
      {
        id: "custom",
        label: "CUSTOM",
        sounds: instance.sounds.filter(
          (sound) => collectionFor(sound) === "custom",
        ),
      },
    ];
    const collectionTabs = node("nav", "collection-tabs");
    collectionTabs.setAttribute("aria-label", "Program collections");
    collections.forEach((collection) => {
      const tab = node(
        "button",
        `collection-tab${collection.id === activeCollection ? " active" : ""}`,
      );
      tab.type = "button";
      tab.setAttribute(
        "aria-current",
        collection.id === activeCollection ? "page" : "false",
      );
      tab.append(
        node("strong", "", collection.label),
        node("span", "", String(collection.sounds.length)),
      );
      tab.addEventListener("click", () => {
        activeCollection = collection.id;
        searchQuery = "";
        renderPlay(instance);
      });
      collectionTabs.append(tab);
    });
    root.append(collectionTabs);

    const currentCard = node("section", "current-program");
    const currentCopy = node("div");
    currentCopy.append(
      node("span", "eyebrow", "CURRENT PROGRAM"),
      node("h2", "", current?.name ?? "No program selected"),
      node("p", "", current?.detail ?? current?.bank ?? "DLS"),
    );
    currentCard.append(currentCopy, node("span", "pulse", "MIDI READY"));
    root.append(currentCard);

    const browser = node("section", "program-browser");
    const toolbar = node("div", "toolbar");
    const search = node("input");
    search.type = "search";
    search.value = searchQuery;
    search.placeholder =
      activeCollection === "custom"
        ? "Search custom programs"
        : "Search DLS programs";
    search.setAttribute("aria-label", "Search programs");
    const activeSounds =
      collections.find((collection) => collection.id === activeCollection)
        ?.sounds ?? [];
    const count = node("span", "count", `${activeSounds.length} PROGRAMS`);
    toolbar.append(search, count);
    const list = node("div", "program-list");
    browser.append(toolbar, list);
    root.append(browser);

    function drawList() {
      const query = search.value.trim().toLowerCase();
      searchQuery = search.value;
      const sounds = activeSounds.filter(
        (sound) =>
          !query ||
          sound.name.toLowerCase().includes(query) ||
          (sound.detail ?? "").toLowerCase().includes(query),
      );
      count.textContent = `${sounds.length} PROGRAMS`;
      list.replaceChildren();
      if (sounds.length === 0) {
        list.append(
          node(
            "p",
            "empty-collection",
            activeCollection === "custom"
              ? "No custom programs yet."
              : "No DLS programs match this search.",
          ),
        );
        return;
      }
      sounds.forEach((sound, index) => {
        const button = node(
          "button",
          `program-row${sound.id === instance.selected_sound_id ? " selected" : ""}`,
        );
        const number = node(
          "span",
          "program-number",
          String(index + 1).padStart(3, "0"),
        );
        const name = node("span", "program-name");
        name.append(
          node("strong", "", sound.name),
          node("small", "", sound.detail ?? sound.bank ?? "DLS"),
        );
        const status = node(
          "span",
          "program-status",
          sound.id === instance.selected_sound_id ? "PLAYING" : "LOAD",
        );
        button.append(number, name, status);
        button.addEventListener("click", () =>
          request("plugin.select_sound", { sound_id: sound.id }),
        );
        list.append(button);
      });
    }

    search.addEventListener("input", drawList);
    drawList();
  }

  function renderConfigLibrary(instance) {
    root.replaceChildren();
    root.append(pluginHeader(instance, "CONFIG SURFACE"));

    const customPrograms = instance.sounds.filter(
      (sound) => collectionFor(sound) === "custom",
    );
    const heading = node("section", "config-library-heading");
    const headingCopy = node("div");
    headingCopy.append(
      node("span", "eyebrow", "CUSTOM PROGRAMS"),
      node("h2", "", "Program editor"),
      node(
        "p",
        "",
        "Create or edit a custom program using the same configuration model as the hardware display.",
      ),
    );
    const addButton = node("button", "editor-primary", "ADD NEW");
    addButton.type = "button";
    addButton.addEventListener("click", () =>
      request("plugin.begin_program_edit", { program_id: null }),
    );
    heading.append(headingCopy, addButton);
    root.append(heading);

    const browser = node("section", "program-browser config-browser");
    const toolbar = node("div", "toolbar");
    const search = node("input");
    search.type = "search";
    search.value = configSearchQuery;
    search.placeholder = "Search custom programs";
    search.setAttribute("aria-label", "Search custom programs");
    const count = node("span", "count", `${customPrograms.length} CUSTOM`);
    toolbar.append(search, count);
    const list = node("div", "program-list");
    browser.append(toolbar, list);
    root.append(browser);

    function drawList() {
      configSearchQuery = search.value;
      const query = configSearchQuery.trim().toLowerCase();
      const filtered = customPrograms.filter(
        (sound) =>
          !query ||
          sound.name.toLowerCase().includes(query) ||
          (sound.detail ?? "").toLowerCase().includes(query),
      );
      count.textContent = `${filtered.length} CUSTOM`;
      list.replaceChildren();
      if (filtered.length === 0) {
        list.append(
          node(
            "p",
            "empty-collection",
            customPrograms.length
              ? "No custom programs match this search."
              : "No custom programs yet. Use ADD NEW to create one.",
          ),
        );
        return;
      }
      filtered.forEach((sound, index) => {
        const button = node("button", "program-row");
        const number = node(
          "span",
          "program-number",
          String(index + 1).padStart(3, "0"),
        );
        const name = node("span", "program-name");
        name.append(
          node("strong", "", sound.name),
          node("small", "", sound.detail ?? "CUSTOM PROGRAM"),
        );
        button.append(number, name, node("span", "program-status", "EDIT"));
        button.addEventListener("click", () =>
          request("plugin.begin_program_edit", { program_id: sound.id }),
        );
        list.append(button);
      });
    }
    search.addEventListener("input", drawList);
    drawList();
  }

  function valueText(value, kind) {
    if (value.type === "inherited") return "INHERITED";
    if (value.type === "boolean") return value.value ? "ON" : "OFF";
    if (value.type === "choice" || value.type === "sound_id") return value.value;
    const decimals = kind.decimals ?? 0;
    const scale = 10 ** decimals;
    return `${(value.value / scale).toFixed(decimals)}${kind.unit ? ` ${kind.unit}` : ""}`;
  }

  function renderToggleField(draft, field, value, disabled) {
    const control = node("button", `toggle-field${value.value ? " active" : ""}`);
    control.type = "button";
    control.disabled = disabled;
    control.setAttribute("role", "switch");
    control.setAttribute("aria-checked", String(value.value));
    control.append(
      node("span", "toggle-track"),
      node("strong", "", value.value ? "ON" : "OFF"),
    );
    control.addEventListener("click", () =>
      commitField(draft, field, {
        type: "boolean",
        value: !value.value,
      }),
    );
    return control;
  }

  function renderNumberField(draft, field, value, disabled) {
    const wrapper = node("div", "number-field");
    const inherited = value.type === "inherited";
    if (field.kind.allow_inherited) {
      const inheritButton = node(
        "button",
        `inherit-button${inherited ? " active" : ""}`,
        inherited ? "INHERITED" : "OVERRIDE",
      );
      inheritButton.type = "button";
      inheritButton.disabled = disabled;
      inheritButton.addEventListener("click", () => {
        if (inherited) {
          const initial = Math.min(
            field.kind.maximum,
            Math.max(field.kind.minimum, 0),
          );
          commitField(draft, field, { type: "integer", value: initial });
        } else {
          commitField(draft, field, { type: "inherited" });
        }
      });
      wrapper.append(inheritButton);
    }
    const rawValue = inherited
      ? Math.min(field.kind.maximum, Math.max(field.kind.minimum, 0))
      : value.value;
    const rangeRow = node("div", "range-row");
    const range = node("input");
    range.type = "range";
    range.min = String(field.kind.minimum);
    range.max = String(field.kind.maximum);
    range.step = String(field.kind.step);
    range.value = String(rawValue);
    range.disabled = disabled || inherited;
    const output = node(
      "output",
      "",
      valueText({ type: "integer", value: rawValue }, field.kind),
    );
    range.addEventListener("input", () => {
      const next = { type: "integer", value: Number(range.value) };
      output.textContent = valueText(next, field.kind);
      if (field.live_preview) previewField(draft, field, next);
      else localFieldValues.set(field.id, next);
    });
    range.addEventListener("change", () =>
      commitField(draft, field, {
        type: "integer",
        value: Number(range.value),
      }),
    );
    rangeRow.append(range, output);
    wrapper.append(rangeRow);
    return wrapper;
  }

  function renderChoiceField(draft, field, value, disabled) {
    const select = node("select", "choice-field");
    select.disabled = disabled;
    field.kind.options.forEach((option) => {
      const element = node("option", "", option.label);
      element.value = option.value;
      element.selected = option.value === value.value;
      select.append(element);
    });
    select.addEventListener("change", () =>
      commitField(draft, field, {
        type: "choice",
        value: select.value,
      }),
    );
    return select;
  }

  function renderSoundField(draft, field, value, disabled, instance) {
    const select = node("select", "choice-field sound-field");
    select.disabled = disabled;
    const sounds = instance.sounds.filter(
      (sound) =>
        !field.kind.bank ||
        (sound.bank ?? "").toLowerCase() === field.kind.bank.toLowerCase(),
    );
    sounds.forEach((sound) => {
      const element = node("option", "", sound.name);
      element.value = sound.id;
      element.selected = sound.id === value.value;
      select.append(element);
    });
    select.addEventListener("change", () =>
      commitField(draft, field, {
        type: "sound_id",
        value: select.value,
      }),
    );
    return select;
  }

  function renderEditorField(draft, field, disabled, instance) {
    const value = displayedFieldValue(field);
    const row = node("div", `editor-field${disabled ? " disabled" : ""}`);
    const copy = node("div", "editor-field-copy");
    copy.append(
      node("strong", "", field.label),
      node("small", "", field.detail),
    );
    let control;
    if (field.kind.type === "toggle" && value.type === "boolean") {
      control = renderToggleField(draft, field, value, disabled);
    } else if (
      field.kind.type === "number" &&
      (value.type === "integer" || value.type === "inherited")
    ) {
      control = renderNumberField(draft, field, value, disabled);
    } else if (field.kind.type === "choice" && value.type === "choice") {
      control = renderChoiceField(draft, field, value, disabled);
    } else if (field.kind.type === "sound" && value.type === "sound_id") {
      control = renderSoundField(draft, field, value, disabled, instance);
    } else {
      control = node("span", "invalid-field", "UNAVAILABLE");
    }
    row.append(copy, control);
    return row;
  }

  function renderEditorPage(
    draft,
    page,
    instance,
    parentEnabled,
    editable,
    depth = 0,
  ) {
    const effectiveEnabled = parentEnabled && page.enabled;
    const section = node(
      "section",
      `editor-section depth-${Math.min(depth, 2)}${effectiveEnabled ? "" : " disabled"}`,
    );
    const heading = node("header", "editor-section-heading");
    const copy = node("div");
    copy.append(
      node("h3", "", page.label),
      node("p", "", page.detail),
    );
    heading.append(
      copy,
      node("span", "section-scope", effectiveEnabled ? "ACTIVE" : "DISABLED"),
    );
    section.append(heading);

    const fields = node("div", "editor-fields");
    (page.fields ?? []).forEach((field) =>
      fields.append(
        renderEditorField(
          draft,
          field,
          !parentEnabled || !editable,
          instance,
        ),
      ),
    );
    if (fields.childElementCount) section.append(fields);

    const children = node("div", "editor-subsections");
    (page.pages ?? []).forEach((child) =>
      children.append(
        renderEditorPage(
          draft,
          child,
          instance,
          effectiveEnabled,
          editable,
          depth + 1,
        ),
      ),
    );
    if (children.childElementCount) section.append(children);
    return section;
  }

  function renderProgramEditor(instance, draft) {
    root.replaceChildren();
    root.append(pluginHeader(instance, "CONFIG SURFACE"));

    if (activeDraftId !== draft.draft_id) {
      activeDraftId = draft.draft_id;
      editMode = false;
      localProgramName = null;
      localFieldValues.clear();
      discardPrompt = false;
      bridgeError = "";
    }
    if (localProgramName === draft.name) localProgramName = null;

    const editorHeader = node("section", "program-editor-header");
    const nameGroup = node("label", "program-name-editor");
    nameGroup.append(node("span", "eyebrow", "PROGRAM NAME"));
    const nameInput = node("input");
    nameInput.type = "text";
    nameInput.maxLength = 64;
    nameInput.value = localProgramName ?? draft.name;
    nameInput.readOnly = !editMode;
    nameInput.className = editMode ? "" : "locked";
    nameInput.setAttribute("aria-label", "Program name");
    const saveName = () => {
      const name = nameInput.value.trim();
      if (!name || name === draft.name) return;
      localProgramName = name;
      request("plugin.set_program_name", {
        draft_id: draft.draft_id,
        name,
      });
    };
    nameInput.addEventListener("change", saveName);
    nameInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        nameInput.blur();
      }
    });
    nameGroup.append(nameInput);

    const actions = node("div", "editor-actions");
    const editToggle = node(
      "button",
      `edit-mode-toggle${editMode ? " active" : ""}`,
      editMode ? "✎ EDITING" : "✎ EDIT MODE",
    );
    editToggle.type = "button";
    editToggle.setAttribute("aria-pressed", String(editMode));
    editToggle.addEventListener("click", () => {
      if (editMode) {
        previewTimers.forEach((timer) => window.clearTimeout(timer));
        previewTimers.clear();
        request("plugin.restore_program_preview", {
          draft_id: draft.draft_id,
        });
      }
      editMode = !editMode;
      renderProgramEditor(instance, draft);
    });
    const status = node(
      "span",
      `draft-status${draft.dirty ? " dirty" : ""}`,
      draft.dirty ? "UNSAVED CHANGES" : "SAVED",
    );
    const cancel = node("button", "editor-secondary", "CANCEL");
    cancel.type = "button";
    cancel.addEventListener("click", () => {
      if (draft.dirty) {
        discardPrompt = true;
        renderProgramEditor(instance, draft);
      } else {
        request("plugin.cancel_program", { draft_id: draft.draft_id });
      }
    });
    const save = node("button", "editor-primary", "SAVE PROGRAM");
    save.type = "button";
    save.addEventListener("click", () => {
      saveName();
      request("plugin.save_program", { draft_id: draft.draft_id });
    });
    actions.append(editToggle, status, cancel, save);
    editorHeader.append(nameGroup, actions);
    root.append(editorHeader);

    if (discardPrompt) {
      const prompt = node("section", "discard-prompt");
      const copy = node("div");
      copy.append(
        node("strong", "", "Discard unsaved changes?"),
        node("p", "", "The saved program will remain unchanged."),
      );
      const buttons = node("div");
      const keep = node("button", "editor-secondary", "KEEP EDITING");
      keep.type = "button";
      keep.addEventListener("click", () => {
        discardPrompt = false;
        renderProgramEditor(instance, draft);
      });
      const discard = node("button", "editor-danger", "DISCARD");
      discard.type = "button";
      discard.addEventListener("click", () =>
        request("plugin.cancel_program", { draft_id: draft.draft_id }),
      );
      buttons.append(keep, discard);
      prompt.append(copy, buttons);
      root.append(prompt);
    }

    if (bridgeError) {
      root.append(node("p", "bridge-error", bridgeError));
    }

    const pages = node(
      "div",
      `program-editor-pages${editMode ? "" : " locked"}`,
    );
    draft.editor.pages.forEach((page) =>
      pages.append(renderEditorPage(draft, page, instance, true, editMode)),
    );
    root.append(pages);
  }

  function renderConfig(instance) {
    const draft = context.program_draft;
    if (draft) renderProgramEditor(instance, draft);
    else {
      activeDraftId = null;
      editMode = false;
      localProgramName = null;
      localFieldValues.clear();
      discardPrompt = false;
      renderConfigLibrary(instance);
    }
  }

  function render() {
    if (!context) return;
    if (surface === "config") renderConfig(context.instance);
    else renderPlay(context.instance);
  }

  window.addEventListener("message", (event) => {
    if (
      event.source !== window.parent ||
      event.origin !== hostOrigin ||
      !event.data ||
      event.data.protocol !== PROTOCOL
    ) {
      return;
    }
    if (event.data.kind === "context") {
      const previousSelection = context?.instance?.selected_sound_id;
      context = event.data;
      if (previousSelection !== context.instance.selected_sound_id) {
        activeCollection = collectionFor(selectedSound(context.instance));
      }
      render();
    } else if (event.data.kind === "response" && !event.data.ok) {
      bridgeError = event.data.error || "RackForge rejected this change.";
      render();
    }
  });

  window.parent.postMessage(
    { protocol: PROTOCOL, kind: "ready" },
    hostOrigin,
  );
})();
