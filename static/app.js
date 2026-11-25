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
