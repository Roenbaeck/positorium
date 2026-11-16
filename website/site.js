import { examples } from "./examples.js";
import { QueryRunner } from "./runner-client.js";

const $ = (id) => document.getElementById(id);
const editor = $("query-editor");
const runButton = $("run-query");
const stopButton = $("stop-query");
const resetButton = $("reset-query");
const status = $("query-status");
const timing = $("query-timing");
const resultContent = $("result-content");
const tableResult = $("table-result");
const jsonResult = $("json-result");
const errorResult = $("query-error");
const placeholder = $("result-placeholder");
const resultSelect = $("result-set");
const resultControl = $("result-set-control");
const exampleButtons = [...document.querySelectorAll("[data-example]")];
const viewButtons = [...document.querySelectorAll("[data-result-view]")];
const runner = new QueryRunner();
let selectedExample = "current";
let resultView = "table";
let response = null;
let busy = false;

// Highlight only text escaped here; query results always use textContent.
function escapeHTML(text) {
  return text.replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );
}

function highlight() {
  const tokens =
    /\/\*[\s\S]*?\*\/|\/\/[^\n]*|"(?:\\.|[^"\\])*"|'[^']*'|\?[\w]+|\b(?:search|return|order|by|in|effect|via|explain|add|role|posit|and|assert|with|at|limit|where|not|exists|or)\b|\b\d+(?:\.\d+)?%?/g;
  const code = editor.value;
  let result = "";
  let cursor = 0;
  for (const match of code.matchAll(tokens)) {
    result += escapeHTML(code.slice(cursor, match.index));
    const token = match[0];
    const kind = token.startsWith("/")
      ? "comment"
      : /^["']/.test(token)
        ? "string"
        : token.startsWith("?")
          ? "variable"
          : /^\d/.test(token)
            ? "number"
            : "keyword";
    result += `<span class="syntax-${kind}">${escapeHTML(token)}</span>`;
    cursor = match.index + token.length;
  }
  $("query-highlight").firstElementChild.innerHTML =
    result + escapeHTML(code.slice(cursor)) + "\n";
  syncScroll();
}

function syncScroll() {
  $("query-highlight").scrollTop = editor.scrollTop;
  $("query-highlight").scrollLeft = editor.scrollLeft;
}

function clearResults() {
  response = null;
  tableResult.replaceChildren();
  jsonResult.textContent = "";
  errorResult.textContent = "";
  placeholder.hidden = false;
  tableResult.hidden =
    jsonResult.hidden =
    errorResult.hidden =
    resultControl.hidden =
      true;
  timing.textContent = "";
  status.textContent = "Ready when you are.";
  status.parentElement.dataset.error = "false";
}

function loadExample(name) {
  selectedExample = name;
  editor.value = examples[name].query;
  editor.scrollTop = editor.scrollLeft = 0;
  highlight();
  exampleButtons.forEach((button) =>
    button.setAttribute(
      "aria-pressed",
      String(button.dataset.example === name),
    ),
  );
  const description = $("example-description");
  const label = document.createElement("span");
  label.className = "mono";
  label.textContent = "THE QUESTION";
  description.replaceChildren(
    label,
    document.createTextNode(` ${examples[name].question}`),
  );
  clearResults();
}

function setBusy(value) {
  busy = value;
  runButton.disabled = resetButton.disabled = value;
  runButton.textContent = value ? "Running…" : "▶ Run query";
  stopButton.hidden = !value;
  editor.readOnly = value;
  exampleButtons.forEach((button) => {
    button.disabled = value;
  });
  resultContent.setAttribute("aria-busy", String(value));
}

function showResult() {
  if (!response) return;
  placeholder.hidden = errorResult.hidden = true;
  tableResult.hidden = resultView !== "table";
  jsonResult.hidden = resultView !== "json";
  jsonResult.textContent = JSON.stringify(response, null, 2);
  tableResult.replaceChildren();
  const set = response.result_sets[Number(resultSelect.value) || 0];
  if (!set || set.rows.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-results";
    empty.textContent = set
      ? "No matching rows. Try changing your query."
      : "Script completed. Add a search to return results.";
    tableResult.append(empty);
    return;
  }
  // EXPLAIN is an ordinary result cell. Make its JSON readable in the table view.
  if (set.columns.length === 1 && set.columns[0] === "plan") {
    try {
      const plan = JSON.parse(set.rows[0][0].text);
      const pre = document.createElement("pre");
      pre.className = "plan-output";
      pre.textContent = JSON.stringify(plan, null, 2);
      tableResult.append(pre);
      return;
    } catch {
      /* An edited query can return a non-JSON cell named plan. */
    }
  }
  const table = document.createElement("table");
  const caption = document.createElement("caption");
  caption.className = "sr-only";
  caption.textContent = `Query result ${Number(resultSelect.value) + 1}`;
  table.append(caption);
  const header = table.createTHead().insertRow();
  set.columns.forEach((column) => {
    const th = document.createElement("th");
    th.scope = "col";
    th.textContent = column;
    header.append(th);
  });
  const body = table.createTBody();
  set.rows.forEach((row) => {
    const tr = body.insertRow();
    row.forEach((cell, index) => {
      const td = tr.insertCell();
      td.textContent = cell.text;
      td.dataset.kind = cell.kind;
      td.dataset.column = set.columns[index];
      td.title = cell.kind;
    });
  });
  tableResult.append(table);
}

async function runQuery() {
  if (busy) return;
  clearResults();
  setBusy(true);
  status.textContent = "Loading the browser engine and running your query…";
  try {
    const { output, elapsed } = await runner.run(editor.value);
    response = output;
    resultSelect.replaceChildren();
    output.result_sets.forEach((set, index) => {
      const option = document.createElement("option");
      option.value = String(index);
      option.textContent = `Result ${index + 1} · ${set.rows.length} rows`;
      resultSelect.append(option);
    });
    resultControl.hidden = output.result_sets.length <= 1;
    const count = output.result_sets.reduce(
      (total, set) => total + set.rows.length,
      0,
    );
    const limited = output.result_sets.some((set) => set.limited);
    status.textContent = `${count} ${count === 1 ? "row" : "rows"} returned${limited ? " · result limit reached" : " · executed on your device"}`;
    timing.textContent = `${elapsed.toFixed(1)} ms`;
    timing.title =
      "Query execution time, excluding engine download and sample setup";
    showResult();
  } catch (error) {
    placeholder.hidden = tableResult.hidden = jsonResult.hidden = true;
    errorResult.hidden = false;
    errorResult.textContent = error.message || String(error);
    status.textContent = error.message.startsWith("Query stopped")
      ? "Query stopped."
      : "The query could not complete.";
    status.parentElement.dataset.error = "true";
  } finally {
    setBusy(false);
  }
}

editor.addEventListener("input", () => {
  highlight();
  if (response)
    status.textContent = "Query changed. Run it again to update these results.";
});
editor.addEventListener("scroll", syncScroll);
editor.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
    event.preventDefault();
    runQuery();
  }
});
runButton.addEventListener("click", runQuery);
stopButton.addEventListener("click", () => runner.stop());
resetButton.addEventListener("click", () => loadExample(selectedExample));
exampleButtons.forEach((button) =>
  button.addEventListener("click", () => loadExample(button.dataset.example)),
);
viewButtons.forEach((button) =>
  button.addEventListener("click", () => {
    resultView = button.dataset.resultView;
    viewButtons.forEach((other) =>
      other.setAttribute("aria-pressed", String(other === button)),
    );
    showResult();
  }),
);
resultSelect.addEventListener("change", showResult);
window.addEventListener("pagehide", () => runner.stop());

document.querySelectorAll("[data-moment]").forEach((button) =>
  button.addEventListener("click", () => {
    const revised = button.dataset.moment === "revised";
    document.querySelector(".evidence-map").dataset.revised = String(revised);
    $("diagram-value").textContent = revised ? "“needs review”" : "“low risk”";
    $("diagram-certainty").replaceChildren(
      document.createTextNode(revised ? "96" : "95"),
      Object.assign(document.createElement("small"), { textContent: "%" }),
    );
    $("diagram-time").textContent = revised
      ? "ASSERTED 09:31"
      : "ASSERTED 09:05";
    $("diagram-summary").textContent = revised
      ? "The screening feed says high risk at 88%. The analyst now says needs review at 96%. The earlier low risk assertion is retained at 0%."
      : "The screening feed says high risk at 88%. The analyst says low risk at 95%. Both claims are preserved.";
    document
      .querySelectorAll("[data-moment]")
      .forEach((other) =>
        other.setAttribute("aria-pressed", String(other === button)),
      );
  }),
);

$("copy-install").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText("pip install --pre positorium");
    $("copy-status").textContent = "Copied to clipboard";
  } catch {
    $("copy-status").textContent = "Select the command to copy it.";
  }
});

loadExample("current");
runButton.disabled = false;
if (!globalThis.Worker || !globalThis.WebAssembly) {
  runButton.disabled = true;
  status.textContent =
    "This playground requires a browser with WebAssembly and Web Workers.";
}
