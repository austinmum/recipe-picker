// Small bits of behaviour for the pages. No libraries.

// Forms with data-confirm ask before submitting (e.g. Delete).
document.querySelectorAll("form[data-confirm]").forEach((form) => {
  form.addEventListener("submit", (e) => {
    if (!confirm(form.dataset.confirm)) e.preventDefault();
  });
});

// ----- Recipe form: add / remove ingredient rows -----
const ingBody = document.getElementById("ing-body");
if (ingBody) {
  const template = document.getElementById("ing-template");
  document.getElementById("add-row").addEventListener("click", () => {
    const row = template.content.firstElementChild.cloneNode(true);
    ingBody.appendChild(row);
    row.querySelector("input[name=ing_quantity]").focus();
  });
  ingBody.addEventListener("click", (e) => {
    if (e.target.classList.contains("remove-row")) e.target.closest("tr").remove();
  });
  ingBody.addEventListener("change", (e) => {
    if (e.target.classList.contains("est-select")) {
      e.target.closest("tr").classList.toggle("estimated", e.target.value === "1");
    }
  });
}

// ----- Photo import: show progress while the AI reads the photos -----
const importForm = document.getElementById("import-form");
if (importForm) {
  const input = importForm.querySelector("input[type=file]");
  const preview = document.getElementById("previews");
  input.addEventListener("change", () => {
    preview.innerHTML = "";
    for (const file of input.files) {
      const img = document.createElement("img");
      img.src = URL.createObjectURL(file);
      img.alt = file.name;
      preview.appendChild(img);
    }
  });
  importForm.addEventListener("submit", () => {
    const button = importForm.querySelector("button[type=submit]");
    button.disabled = true;
    button.textContent = "Reading your recipe… (this can take up to a minute)";
    document.getElementById("working").hidden = false;
  });
}

// ----- This week: filter the recipe list -----
const weekFilter = document.getElementById("week-filter");
if (weekFilter) {
  const rows = document.querySelectorAll(".pick-row");
  const counter = document.getElementById("pick-count");
  const update = () => {
    const picked = document.querySelectorAll(".pick-row input[value='1']:checked, .pick-row input[value='2']:checked").length;
    counter.textContent = picked;
    recommend();
  };
  // Recommend unpicked recipes that share perishable produce with a pick
  // but don't use a protein you've already picked.
  const list = (row, key) => (row.dataset[key] ? row.dataset[key].split("|") : []);
  const recommend = () => {
    const picks = [...rows].filter((r) => r.classList.contains("picked"));
    const produce = new Set(picks.flatMap((r) => list(r, "produce")));
    const protein = new Set(picks.flatMap((r) => list(r, "protein")));
    rows.forEach((r) => {
      const shared = r.classList.contains("picked") ? [] : list(r, "produce").filter((p) => produce.has(p));
      const sameProtein = list(r, "protein").some((p) => protein.has(p));
      const on = shared.length > 0 && !sameProtein;
      const badge = r.querySelector(".rec");
      badge.hidden = !on;
      r.classList.toggle("recommended", on);
      if (on) badge.querySelector(".rec-why").textContent = "uses your " + shared.join(", ");
    });
  };
  weekFilter.addEventListener("input", () => {
    const q = weekFilter.value.trim().toLowerCase();
    rows.forEach((r) => (r.hidden = q && !r.dataset.search.includes(q)));
  });
  document.querySelectorAll(".pick-row input[type=radio]").forEach((r) => r.addEventListener("change", (e) => {
    e.target.closest(".pick-row").classList.toggle("picked", e.target.value !== "0");
    update();
  }));
  update();
}

// ----- Shopping list: save ticks as you click -----
document.querySelectorAll(".list-item input[type=checkbox]").forEach((box) => {
  box.addEventListener("change", async () => {
    const item = box.closest(".list-item");
    item.classList.toggle(box.dataset.field, box.checked);
    const body = new URLSearchParams({ key: item.dataset.key, field: box.dataset.field, value: box.checked ? "1" : "0", amount: item.dataset.amount });
    try {
      const res = await fetch("/list/check", { method: "POST", body });
      if (!res.ok) throw new Error(await res.text());
    } catch (err) {
      box.checked = !box.checked;
      item.classList.toggle(box.dataset.field, box.checked);
      alert("Couldn't save that tick. Is the app still running?");
    }
    updateProgress();
  });
});

function updateProgress() {
  document.querySelectorAll(".trip").forEach((trip) => {
    const items = trip.querySelectorAll(".list-item");
    const done = trip.querySelectorAll(".list-item.have, .list-item.got").length;
    const out = trip.querySelector(".progress");
    if (out) out.textContent = items.length ? `${done} of ${items.length} done` : "";
  });
}
updateProgress();

// Hide items already sorted (have it / got it).
const hideDone = document.getElementById("hide-done");
if (hideDone) {
  const apply = () => document.body.classList.toggle("hide-done", hideDone.checked);
  hideDone.addEventListener("change", apply);
  apply();
}

// ----- Cooking mode -----
const cook = document.getElementById("cook");
if (cook) {
  // Remember ticks per recipe while you cook (in this browser only).
  const storeKey = `cook-${cook.dataset.recipe}`;
  const load = () => { try { return JSON.parse(sessionStorage.getItem(storeKey)) || {}; } catch { return {}; } };
  const save = (s) => { try { sessionStorage.setItem(storeKey, JSON.stringify(s)); } catch {} };
  const state = load();
  const ticks = [...cook.querySelectorAll(".tick")];
  const steps = [...cook.querySelectorAll(".step")];

  const render = () => {
    ticks.forEach((t, i) => t.classList.toggle("done", !!(state.ing || [])[i]));
    steps.forEach((s, i) => s.classList.toggle("done", !!(state.steps || [])[i]));
    const next = steps.find((s) => !s.classList.contains("done"));
    steps.forEach((s) => s.classList.toggle("current", s === next));
    cook.querySelector(".all-done").hidden = !!next || steps.length === 0;
  };
  const toggle = (kind, i) => {
    state[kind] = state[kind] || [];
    state[kind][i] = !state[kind][i];
    save(state);
    render();
  };
  ticks.forEach((t, i) => t.addEventListener("click", () => toggle("ing", i)));
  steps.forEach((s, i) => s.addEventListener("click", () => {
    const wasCurrent = s.classList.contains("current");
    toggle("steps", i);
    if (wasCurrent) {
      const next = cook.querySelector(".step.current");
      if (next) next.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }));
  render();

  // Text size, remembered between visits.
  let size = 1.25;
  try { size = parseFloat(localStorage.getItem("cook-size")) || size; } catch {}
  const setSize = (v) => {
    size = Math.min(2.2, Math.max(0.9, v));
    document.body.style.setProperty("--cook-size", `${size}rem`);
    try { localStorage.setItem("cook-size", size); } catch {}
  };
  setSize(size);
  document.getElementById("text-bigger").addEventListener("click", () => setSize(size + 0.15));
  document.getElementById("text-smaller").addEventListener("click", () => setSize(size - 0.15));

  // Keep the screen awake while cooking (where the browser supports it).
  const awake = document.getElementById("keep-awake");
  let lock = null;
  const requestLock = async () => {
    if (!awake.checked || !("wakeLock" in navigator) || document.visibilityState !== "visible") return;
    try { lock = await navigator.wakeLock.request("screen"); } catch { lock = null; }
  };
  if (!("wakeLock" in navigator)) {
    awake.parentElement.title = "This browser can't keep the screen on";
    awake.disabled = true;
    awake.checked = false;
  }
  awake.addEventListener("change", () => { if (awake.checked) requestLock(); else if (lock) { lock.release(); lock = null; } });
  document.addEventListener("visibilitychange", requestLock);
  requestLock();
}
