/*
       * A minimal RackForge plugin surface.
       *
       * It owns its own interface and asks the host for everything else: the
       * parameter schema, the current values, and permission to change one.
       * No path, no plugin id and no Core command is ever sent from here — the
       * host derives all of that from which surface is talking to it.
       */
      const PROTOCOL = "rackforge.plugin.web@1";
      const pending = new Map();
      let nextRequest = 1;

      const controls = document.getElementById("controls");
      // The workbench tools are for development, not the instrument's face:
      // they exist only with ?dev=1 in the URL, or after Ctrl+Shift+D
      // (remembered by the panel until toggled off the same way).
      // A host may deny storage entirely; the panel must still play.
      const storage = {
        get(key) {
          try { return localStorage.getItem(key); } catch { return null; }
        },
        set(key, value) {
          try { localStorage.setItem(key, value); } catch { /* volatile */ }
        },
      };
      let devMode =
        new URLSearchParams(location.search).has("dev") ||
        storage.get("cg-dev") === "1";
      window.addEventListener("keydown", (event) => {
        if (event.ctrlKey && event.shiftKey && event.code === "KeyD") {
          devMode = !devMode;
          storage.set("cg-dev", devMode ? "1" : "0");
          state.textContent = devMode ? "Dev tools ON" : "Dev tools OFF";
          void load(false);
        }
      });
      const voicing = document.getElementById("voicing");
      const state = document.getElementById("state");

      // The splash ships in the package beside this surface: 1920x1080,
      // made for a full screen, so the page's whole background is the
      // instrument itself without the crop a 4:1 banner would need.
      document.body.style.backgroundImage = "url(../branding/splash.png)";

      function call(method, params = {}) {
        const requestId = `concert-grand-${nextRequest++}`;
        return new Promise((resolve, reject) => {
          pending.set(requestId, { resolve, reject });
          parent.postMessage(
            { protocol: PROTOCOL, kind: "request", request_id: requestId, method, params },
            "*",
          );
        });
      }

      window.addEventListener("message", (event) => {
        const message = event.data;
        if (!message || message.protocol !== PROTOCOL) return;
        if (message.kind === "context") {
          state.textContent = `Connected · ${message.surface ?? "play"} surface`;
          selectedSoundId = message.instance?.selected_sound_id ?? selectedSoundId;
          void load();
          return;
        }
        if (message.kind !== "response") return;
        const waiting = pending.get(message.request_id);
        if (!waiting) return;
        pending.delete(message.request_id);
        if (message.ok) {
          waiting.resolve(message.result);
        } else {
          waiting.reject(new Error(message.error ?? "RackForge refused the request"));
        }
      });

      /** One fader, wired to the host and painted to its own fill. */
      function fader(parameter, value) {
        const kind = parameter.kind;
        const wrapper = document.createElement("div");
        wrapper.className = "control";
        const row = document.createElement("div");
        row.className = "row";
        const name = document.createElement("span");
        name.className = "name";
        name.textContent = parameter.name;
        const reading = document.createElement("span");
        reading.className = "value";
        row.append(name, reading);

        // A logarithmic control moves the slider through equal RATIOS rather
        // than equal steps, so the travel is spent where a player works. Mic
        // Distance runs half a metre to sixteen and Room Size forty-five cubic
        // metres to forty-five thousand: drawn linearly, everything anyone
        // would choose sits in the first fraction of the track. The slider
        // therefore carries a position from nought to one and the value is
        // derived from it; only the plugin's descriptor knows which of the two
        // a parameter is.
        const ratio = kind.taper === "logarithmic" && kind.minimum > 0
          ? kind.maximum / kind.minimum
          : 0;
        const toValue = (position) =>
          ratio ? kind.minimum * Math.pow(ratio, position) : position;
        const toPosition = (magnitude) =>
          ratio
            ? Math.log(Math.max(magnitude, kind.minimum) / kind.minimum) / Math.log(ratio)
            : magnitude;

        const slider = document.createElement("input");
        slider.type = "range";
        slider.min = String(ratio ? 0 : kind.minimum);
        slider.max = String(ratio ? 1 : kind.maximum);
        slider.step = String(ratio ? 0.001 : (kind.step ?? 0.01));
        slider.value = String(toPosition(value ?? kind.default));
        slider.setAttribute("aria-label", parameter.name);

        /** Enough figures to see a change, never more than the value carries. */
        const span = Math.abs(kind.maximum - kind.minimum) || 1;
        const format = (magnitude) => {
          // Small against its OWN range is zero, whatever the arithmetic says.
          // A saved session can hold a fader an ulp off centre, and on a
          // control running -24 to +24 dB that came out as "-1.28e-6 dB":
          // true, alarming, and inaudible.
          const size = Math.abs(magnitude) < span * 1e-6 ? 0 : Math.abs(magnitude);
          let text;
          if (size >= 0.01 || size === 0) {
            text = (size === 0 ? 0 : magnitude).toFixed(
              size >= 100 ? 0 : size >= 10 ? 1 : 2,
            );
          } else {
            // A model constant can be a wire gauge of 0.00123 m or a squared
            // magnitude of 3e-10, and two decimals render both as "0.00" -- a
            // control that is doing something, reading as off. Three figures
            // of it, in whichever notation carries them.
            const decimals = Math.ceil(-Math.log10(size)) + 2;
            text =
              decimals <= 6 ? magnitude.toFixed(decimals) : magnitude.toExponential(2);
          }
          return kind.unit ? `${text} ${kind.unit}` : text;
        };

        const show = () => {
          const position = Number(slider.value);
          const low = ratio ? 0 : kind.minimum;
          const span = (ratio ? 1 : kind.maximum) - low || 1;
          slider.style.setProperty("--fill", `${((position - low) / span) * 100}%`);
          reading.textContent = format(toValue(position));
        };
        show();
        slider.addEventListener("input", () => {
          show();
          void call("plugin.set_parameter", {
            parameter_index: parameter.index,
            value: toValue(Number(slider.value)),
          }).catch((error) => {
            state.textContent = error.message;
          });
        });

        wrapper.append(row, slider);
        return wrapper;
      }

      /**
       * One choice among a few, for the things about a piano that are A or B.
       *
       * A fader says "somewhere between"; a piano's action is a grand's or an
       * upright's and there is no instrument in the middle. So an enum is not
       * a dropdown and not a slider with two stops -- it is a row of plates,
       * one of them lit, the same idiom the tabs on the rail already use for
       * "one of several, one active".
       */
      function selector(parameter, value) {
        const kind = parameter.kind;
        const wrapper = document.createElement("div");
        wrapper.className = "control";
        const row = document.createElement("div");
        row.className = "row";
        const name = document.createElement("span");
        name.className = "name";
        name.textContent = parameter.name;
        const reading = document.createElement("span");
        reading.className = "value";
        row.append(name, reading);

        const segment = document.createElement("div");
        // A row of two or three plates reads at the tab's size; a row of
        // thirteen keys wants smaller plates, or it stands twice as tall
        // as the fader beside it for no reason.
        segment.className = kind.choices.length > 4 ? "segment dense" : "segment";
        segment.setAttribute("role", "radiogroup");
        segment.setAttribute("aria-label", parameter.name);
        let chosen = Number(value ?? kind.default);
        const buttons = kind.choices.map((choice) => {
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = choice.name;
          button.setAttribute("role", "radio");
          button.addEventListener("click", () => {
            if (choice.value === chosen) return;
            chosen = choice.value;
            show();
            void call("plugin.set_parameter", {
              parameter_index: parameter.index,
              value: choice.value,
            }).catch((error) => {
              state.textContent = error.message;
            });
          });
          return { button, choice };
        });
        const show = () => {
          for (const { button, choice } of buttons) {
            const on = choice.value === chosen;
            button.setAttribute("aria-checked", on ? "true" : "false");
          }
          const current = kind.choices.find((choice) => choice.value === chosen);
          reading.textContent = current ? current.name : String(chosen);
        };
        show();
        segment.append(...buttons.map(({ button }) => button));
        wrapper.append(row, segment);
        return wrapper;
      }

      /** A grid of controls for one page: faders, and plates where the thing is A or B. */
      function faderGrid(parameters, current) {
        const grid = document.createElement("div");
        grid.className = "grid";
        for (const parameter of parameters) {
          const value = current.get(parameter.index);
          grid.append(
            parameter.kind.type === "enum" ? selector(parameter, value) : fader(parameter, value),
          );
        }
        return grid;
      }

      // The tab the player left open survives preset changes and reloads;
      // so does the sector open inside the Model tab.
      let activeTab = null;
      let activeModelTab = null;

      /**
       * The panel as tabbed pages of one instrument: the main pages each get
       * a tab on the walnut rail, and the Lab's themed groups share one Lab
       * tab at the end.
       */
      function draw(schema, values) {
        const current = new Map(values.map((value) => [value.index, value.value]));
        const pages = [...(schema.pages ?? [])].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
        // Faders for the continuous things, plates for the ones that are A or
        // B. Everything else -- meters, triggers -- has no place on a piano's
        // own panel and is left to the host.
        const floats = schema.parameters.filter(
          (p) => p.kind.type === "float" || p.kind.type === "enum",
        );
        const forPage = (id) =>
          floats
            .filter((p) => (p.page ?? null) === id)
            .sort((a, b) => (a.order ?? 0) - (b.order ?? 0));

        const tabs = [];
        const labGroups = [];
        const modelGroups = [];
        for (const page of pages.length ? pages : [{ id: null, name: "Voice" }]) {
          const mine = forPage(page.id);
          if (!mine.length) continue;
          if (page.id && page.id.startsWith("lab")) {
            labGroups.push({ page, mine });
          } else if (page.id && page.id.startsWith("model")) {
            modelGroups.push({ page, mine });
          } else {
            const body = document.createElement("div");
            // Every page gets the same screwed-down plate the lab groups get;
            // otherwise the brass would look like a workbench decoration.
            const plate = document.createElement("div");
            plate.className = "labgroup";
            plate.append(faderGrid(mine, current));
            body.append(plate);
            tabs.push({ id: page.id ?? "voice", name: page.name, body });
          }
        }
        // The model's own constants, one tab with a sector strip inside it:
        // Strings, Hammer, Board, Air & Mics, Misc. Ninety-odd faders would
        // bury the voicing pages if they sat on the rail one sector each.
        if (modelGroups.length) {
          const body = document.createElement("div");
          const strip = document.createElement("nav");
          strip.className = "subtabs";
          strip.setAttribute("role", "tablist");
          const holder = document.createElement("div");
          const sectors = modelGroups.map(({ page, mine }) => {
            const plate = document.createElement("div");
            plate.className = "labgroup";
            plate.append(faderGrid(mine, current));
            return { id: page.id, name: page.name.replace(/^Model\s*·\s*/, ""), plate };
          });
          if (!sectors.some((sector) => sector.id === activeModelTab)) {
            activeModelTab = sectors[0].id;
          }
          const choose = (id) => {
            activeModelTab = id;
            for (const sector of sectors) {
              sector.button.setAttribute("aria-selected", String(sector.id === id));
            }
            const chosen = sectors.find((sector) => sector.id === id);
            holder.replaceChildren(chosen ? chosen.plate : document.createElement("div"));
          };
          for (const sector of sectors) {
            const button = document.createElement("button");
            button.type = "button";
            button.setAttribute("role", "tab");
            button.textContent = sector.name;
            button.addEventListener("click", () => choose(sector.id));
            sector.button = button;
            strip.append(button);
          }
          // The voicings carry the forty-one voicing parameters and leave
          // the model's constants where the player left them, so the way
          // back to the compiled instrument is a button, not a preset.
          const reset = document.createElement("div");
          reset.className = "resetrow";
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = "Reset model to compiled values";
          button.addEventListener("click", async () => {
            button.disabled = true;
            try {
              for (const { mine } of modelGroups) {
                for (const parameter of mine) {
                  await call("plugin.set_parameter", {
                    parameter_index: parameter.index,
                    value: parameter.kind.default ?? 0.5,
                  });
                }
              }
              state.textContent = "Model · compiled values";
              await load(false);
            } catch (error) {
              state.textContent = error.message;
            } finally {
              button.disabled = false;
            }
          });
          reset.append(button);
          body.append(strip, holder, reset);
          choose(activeModelTab);
          tabs.push({ id: "model", name: "Model", body });
        }
        if (labGroups.length) {
          const body = document.createElement("div");
          for (const { page, mine } of labGroups) {
            const group = document.createElement("div");
            group.className = "labgroup";
            const heading = document.createElement("h3");
            heading.textContent = page.name.replace(/^Lab\s*·\s*/, "");
            group.append(heading, faderGrid(mine, current));
            body.append(group);
          }
          // Exporting the settings is a development tool: it lives with
          // the workbench controls, and only in dev mode.
          if (devMode) {
            body.append(exportRow());
          }
          tabs.push({ id: "lab", name: "Lab", body });
        }
        // The model's constants are the last thing a player reaches for, so
        // their tab is the last on the rail, past the Lab.
        const modelTab = tabs.findIndex((tab) => tab.id === "model");
        if (modelTab >= 0) {
          tabs.push(...tabs.splice(modelTab, 1));
        }

        const rail = document.createElement("div");
        rail.className = "rail";
        const bar = document.createElement("nav");
        bar.className = "tabs";
        bar.setAttribute("role", "tablist");
        rail.append(bar);
        const panel = document.createElement("div");
        panel.className = "panelbox";

        if (!tabs.some((tab) => tab.id === activeTab)) {
          activeTab = tabs[0]?.id ?? null;
        }
        const select = (id) => {
          activeTab = id;
          for (const tab of tabs) {
            tab.button.setAttribute("aria-selected", String(tab.id === id));
          }
          const chosen = tabs.find((tab) => tab.id === id);
          panel.replaceChildren(chosen ? chosen.body : document.createElement("div"));
        };
        for (const tab of tabs) {
          const button = document.createElement("button");
          button.type = "button";
          button.setAttribute("role", "tab");
          button.textContent = tab.name;
          button.addEventListener("click", () => select(tab.id));
          tab.button = button;
          bar.append(button);
        }
        controls.replaceChildren(rail, panel);
        select(activeTab);
      }

      /**
       * Voicings: the packaged factory presets (loaded through
       * `plugin.select_sound`) and the player's own, stored by the panel
       * and applied parameter by parameter. Cards on the shelf; the user's
       * carry a save (replace with the current values, after asking) and a
       * delete.
       */
      const STORE_KEY = "cg-user-presets";
      let chosenCard = null;

      function userPresets() {
        try {
          return JSON.parse(storage.get(STORE_KEY) ?? "[]");
        } catch {
          return [];
        }
      }
      function saveUserPresets(list) {
        storage.set(STORE_KEY, JSON.stringify(list));
      }

      async function currentValues() {
        const parameters = await call("plugin.parameters");
        const values = {};
        for (const v of parameters.values ?? []) {
          values[v.index] = v.value;
        }
        return values;
      }

      async function applyValues(values) {
        for (const [index, value] of Object.entries(values)) {
          await call("plugin.set_parameter", {
            parameter_index: Number(index),
            value: Number(value),
          });
        }
      }

      function veilConfirm(text, confirmLabel = "Replace") {
        return new Promise((resolve) => {
          const veil = document.getElementById("veil");
          document.getElementById("veiltext").textContent = text;
          const yes = document.getElementById("veilyes");
          yes.textContent = confirmLabel;
          const no = document.getElementById("veilno");
          const close = (answer) => {
            veil.classList.remove("open");
            yes.onclick = no.onclick = null;
            resolve(answer);
          };
          yes.onclick = () => close(true);
          no.onclick = () => close(false);
          veil.classList.add("open");
        });
      }

      // The nameplate is RackForge's program selector, fed by this panel
      // (`source="manual"`): the factory voicings are the host's programs, the
      // player's own are applied here parameter by parameter, and the plate
      // lists both in the shelf's order. Choosing on the plate -- an arrow, or
      // the search it opens -- is choosing the card; the card's own click does
      // the work, so there is one path however a voicing is picked. The plate
      // shrinks a long name to fit (`fit`) and slides it the way the choice
      // moved (`slide`), as the panel's own plate did.
      const programSelector = document.getElementById("program-selector");
      const BANKS = [
        { id: "factory", name: "Factory", order: 0 },
        { id: "user", name: "Saved", order: 1 },
      ];

      function feedSelector() {
        programSelector.banks = BANKS;
        programSelector.programs = [...voicing.querySelectorAll(".card")].map((card) => ({
          id: card.dataset.programId,
          name: card.querySelector(".title")?.textContent?.trim() ?? "",
          bank: card.dataset.bank,
          detail: card.title || undefined,
        }));
        programSelector.value = chosenCard?.dataset.programId ?? null;
      }

      programSelector.addEventListener("rf-program-select", (event) => {
        const card = [...voicing.querySelectorAll(".card")].find(
          (candidate) => candidate.dataset.programId === event.detail.id,
        );
        if (card) card.click();
        else event.preventDefault();
      });

      // A voicing of the player's own is not a host program, so the host's
      // selection still names the factory voicing it started from. Remember
      // which card it was, and on which host selection, so a rebuilt shelf
      // keeps it -- until the host's selection moves, which means something
      // else chose a program.
      let userChoice = null;

      function chooseCard(card) {
        chosenCard = card;
        userChoice =
          card?.dataset.bank === "user"
            ? { programId: card.dataset.programId, soundId: selectedSoundId }
            : null;
        for (const other of voicing.querySelectorAll(".card")) {
          other.setAttribute("aria-pressed", String(other === card));
        }
        programSelector.value = card?.dataset.programId ?? null;
      }

      function presetCard(name, onPick, extras) {
        const card = document.createElement("div");
        card.className = "card";
        card.setAttribute("aria-pressed", "false");
        const title = document.createElement("span");
        title.className = "title";
        title.textContent = name;
        card.append(title);
        if (extras) card.append(extras);
        card.addEventListener("click", (event) => {
          if (event.target.closest(".tools-inline")) return;
          chooseCard(card);
          onPick();
        });
        return card;
      }

      // The host's word on which voicing is selected; the shelf's
      // highlight follows it, so a rebuilt shelf (every fresh context
      // re-renders it) keeps pointing at the card the player chose
      // instead of snapping back to the first one.
      let selectedSoundId = null;

      async function drawPresets() {
        let factory = [];
        try {
          const response = await fetch("../metadata/presets.json");
          factory = (await response.json()).presets ?? [];
        } catch {
          /* the shelf still shows the player's own */
        }
        factory.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
        voicing.replaceChildren();
        for (const preset of factory) {
          const card = presetCard(preset.name ?? preset.id, async () => {
            try {
              await call("plugin.select_sound", { sound_id: preset.id });
              selectedSoundId = preset.id;
              state.textContent = `Voicing · ${preset.name ?? preset.id}`;
              await load(false);
            } catch (error) {
              state.textContent = error.message;
            }
          });
          card.title = preset.description ?? "";
          card.dataset.soundId = preset.id;
          card.dataset.programId = preset.id;
          card.dataset.bank = "factory";
          voicing.append(card);
        }
        for (const preset of userPresets()) {
          const tools = document.createElement("span");
          tools.className = "tools-inline";
          const save = document.createElement("button");
          save.type = "button";
          save.textContent = "\u{1F4BE}";
          save.title = "Save the current values into this preset";
          save.addEventListener("click", async () => {
            if (!(await veilConfirm(`Replace “${preset.name}” with the current values?`))) {
              return;
            }
            try {
              const values = await currentValues();
              const list = userPresets();
              const mine = list.find((p) => p.id === preset.id);
              if (mine) mine.values = values;
              saveUserPresets(list);
              state.textContent = `Saved · ${preset.name}`;
            } catch (error) {
              state.textContent = error.message;
            }
          });
          const trash = document.createElement("button");
          trash.type = "button";
          trash.textContent = "\u{1F5D1}";
          trash.title = "Delete this preset";
          trash.addEventListener("click", async () => {
            if (!(await veilConfirm(`Delete “${preset.name}”?`, "Delete"))) return;
            saveUserPresets(userPresets().filter((p) => p.id !== preset.id));
            await drawPresets();
          });
          tools.append(save, trash);
          const card = presetCard(preset.name, async () => {
            try {
              await applyValues(preset.values);
              state.textContent = `Voicing · ${preset.name}`;
              await load(false);
            } catch (error) {
              state.textContent = error.message;
            }
          }, tools);
          // The player's ids are made here and cannot meet a factory id.
          card.dataset.programId = `user:${preset.id}`;
          card.dataset.bank = "user";
          voicing.append(card);
        }
        // The highlight follows the host's selection, or the player's own
        // voicing applied on top of it; a fresh panel with no word from the
        // host yet falls back to the first factory card, which is the
        // packaged default.
        const cards = [...voicing.querySelectorAll(".card")];
        const chosen =
          (userChoice?.soundId === selectedSoundId &&
            cards.find((card) => card.dataset.programId === userChoice.programId)) ||
          (selectedSoundId && cards.find((card) => card.dataset.soundId === selectedSoundId)) ||
          cards[0];
        if (chosen) chooseCard(chosen);
        // move the shelf furniture after the cards
        const add = document.getElementById("addpreset");
        const namer = document.getElementById("namer");
        add.parentElement.append(add, namer);
        feedSelector();
      }

      {
        const add = document.getElementById("addpreset");
        const namer = document.getElementById("namer");
        const nameField = document.getElementById("presetname");
        add.addEventListener("click", () => {
          namer.classList.add("open");
          nameField.value = "";
          nameField.focus();
        });
        const commit = async () => {
          const name = nameField.value.trim();
          if (!name) return;
          try {
            const values = await currentValues();
            const list = userPresets();
            list.push({ id: `u-${Date.now()}`, name, values });
            saveUserPresets(list);
            namer.classList.remove("open");
            state.textContent = `Saved · ${name}`;
            await drawPresets();
            // The values on the faders ARE this preset: it shows as chosen.
            const cards = voicing.querySelectorAll(".card");
            if (cards.length) chooseCard(cards[cards.length - 1]);
          } catch (error) {
            state.textContent = error.message;
          }
        };
        document.getElementById("namerok").addEventListener("click", commit);
        nameField.addEventListener("keydown", (event) => {
          if (event.key === "Enter") void commit();
          if (event.key === "Escape") namer.classList.remove("open");
        });
      }

      async function load(withPresets = true) {
        try {
          const parameters = await call("plugin.parameters");
          draw(parameters.schema, parameters.values ?? []);
          if (withPresets) await drawPresets();
        } catch (error) {
          state.textContent = error.message;
        }
      }

      /** The workbench export: every value as JSON, for tuning by ear. */
      function exportRow() {
        const tools = document.createElement("div");
        tools.className = "tools";
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = "Copy JSON";
        const dump = document.createElement("textarea");
        dump.readOnly = true;
        tools.append(button);
        const wrapper = document.createElement("div");
        wrapper.append(tools, dump);
        button.addEventListener("click", () => void copySettings(dump));
        return wrapper;
      }

      async function copySettings(dump) {
        try {
          const parameters = await call("plugin.parameters");
          const values = new Map((parameters.values ?? []).map((v) => [v.index, v.value]));
          const out = {};
          // Which build these numbers mean: the host stamps the package
          // version onto this page's URL.
          const stamp = new URLSearchParams(location.search).get("v");
          if (stamp) out._version = stamp;
          // The union of everything: every declared parameter, and any live
          // value the schema does not know (so nothing can be missing).
          const seen = new Set();
          for (const p of parameters.schema.parameters) {
            out[p.id ?? p.name] = Number((values.get(p.index) ?? p.kind.default).toFixed(3));
            seen.add(p.index);
          }
          for (const [index, value] of values) {
            if (!seen.has(index)) {
              out[`param_${index}`] = Number(value.toFixed(3));
            }
          }
          const text = JSON.stringify(out, null, 1);
          dump.style.display = "block";
          dump.value = text;
          dump.select();
          try {
            await navigator.clipboard.writeText(text);
            state.textContent = "Copied to the clipboard ✓";
          } catch {
            document.execCommand("copy");
            state.textContent = "Selected — press Ctrl+C to copy";
          }
        } catch (error) {
          state.textContent = error.message;
        }
      }

      parent.postMessage({ protocol: PROTOCOL, kind: "ready" }, "*");
