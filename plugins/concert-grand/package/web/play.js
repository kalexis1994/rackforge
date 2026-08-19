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

        const slider = document.createElement("input");
        slider.type = "range";
        slider.min = String(kind.minimum);
        slider.max = String(kind.maximum);
        slider.step = String(kind.step ?? 0.01);
        slider.value = String(value ?? kind.default);
        slider.setAttribute("aria-label", parameter.name);

        const show = () => {
          const span = kind.maximum - kind.minimum || 1;
          const fill = ((Number(slider.value) - kind.minimum) / span) * 100;
          slider.style.setProperty("--fill", `${fill}%`);
          reading.textContent = Number(slider.value).toFixed(2);
        };
        show();
        slider.addEventListener("input", () => {
          show();
          void call("plugin.set_parameter", {
            parameter_index: parameter.index,
            value: Number(slider.value),
          }).catch((error) => {
            state.textContent = error.message;
          });
        });

        wrapper.append(row, slider);
        return wrapper;
      }

      /** A grid of faders for one page. */
      function faderGrid(parameters, current) {
        const grid = document.createElement("div");
        grid.className = "grid";
        for (const parameter of parameters) {
          grid.append(fader(parameter, current.get(parameter.index)));
        }
        return grid;
      }

      // The tab the player left open survives preset changes and reloads.
      let activeTab = null;

      /**
       * The panel as tabbed pages of one instrument: the main pages each get
       * a tab on the walnut rail, and the Lab's themed groups share one Lab
       * tab at the end.
       */
      function draw(schema, values) {
        const current = new Map(values.map((value) => [value.index, value.value]));
        const pages = [...(schema.pages ?? [])].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
        const floats = schema.parameters.filter((p) => p.kind.type === "float");
        const forPage = (id) =>
          floats
            .filter((p) => (p.page ?? null) === id)
            .sort((a, b) => (a.order ?? 0) - (b.order ?? 0));

        const tabs = [];
        const labGroups = [];
        for (const page of pages.length ? pages : [{ id: null, name: "Voice" }]) {
          const mine = forPage(page.id);
          if (!mine.length) continue;
          if (page.id && page.id.startsWith("lab")) {
            labGroups.push({ page, mine });
          } else {
            const body = document.createElement("div");
            body.append(faderGrid(mine, current));
            tabs.push({ id: page.id ?? "voice", name: page.name, body });
          }
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

      function veilConfirm(text, confirmLabel = "Reemplazar") {
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

      function chooseCard(card) {
        chosenCard = card;
        for (const other of voicing.querySelectorAll(".card")) {
          other.setAttribute("aria-pressed", String(other === card));
        }
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
              state.textContent = `Voicing · ${preset.name ?? preset.id}`;
              await load(false);
            } catch (error) {
              state.textContent = error.message;
            }
          });
          card.title = preset.description ?? "";
          voicing.append(card);
        }
        // The factory voicing shows as chosen from the start: the packaged
        // default IS the first card, and a fresh panel plays exactly it.
        const first = voicing.querySelector(".card");
        if (first) first.setAttribute("aria-pressed", "true");
        for (const preset of userPresets()) {
          const tools = document.createElement("span");
          tools.className = "tools-inline";
          const save = document.createElement("button");
          save.type = "button";
          save.textContent = "\u{1F4BE}";
          save.title = "Guardar los valores actuales en este preset";
          save.addEventListener("click", async () => {
            if (!(await veilConfirm(`¿Reemplazar “${preset.name}” con los valores actuales?`))) {
              return;
            }
            try {
              const values = await currentValues();
              const list = userPresets();
              const mine = list.find((p) => p.id === preset.id);
              if (mine) mine.values = values;
              saveUserPresets(list);
              state.textContent = `Guardado · ${preset.name}`;
            } catch (error) {
              state.textContent = error.message;
            }
          });
          const trash = document.createElement("button");
          trash.type = "button";
          trash.textContent = "\u{1F5D1}";
          trash.title = "Borrar este preset";
          trash.addEventListener("click", async () => {
            if (!(await veilConfirm(`¿Borrar “${preset.name}”?`, "Borrar"))) return;
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
          voicing.append(card);
        }
        // move the shelf furniture after the cards
        const add = document.getElementById("addpreset");
        const namer = document.getElementById("namer");
        add.parentElement.append(add, namer);
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
            state.textContent = `Guardado · ${name}`;
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
        button.textContent = "Copiar JSON";
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
            state.textContent = "Copiado al portapapeles ✓";
          } catch {
            document.execCommand("copy");
            state.textContent = "Seleccionado — Ctrl+C para copiar";
          }
        } catch (error) {
          state.textContent = error.message;
        }
      }

      parent.postMessage({ protocol: PROTOCOL, kind: "ready" }, "*");
