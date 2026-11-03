"use strict";

const search = document.querySelector("#search");
const rows = document.querySelector("#rows");
const status = document.querySelector("#status");
const previous = document.querySelector("#previous");
const next = document.querySelector("#next");
let offset = 0;
let generation = 0;
let state;
let timer;

function clearPage() {
  generation++;
  state = undefined;
  rows.replaceChildren();
  previous.disabled = true;
  next.disabled = true;
  status.textContent = "";
}

async function refresh() {
  clearPage();
  if (document.hidden) return;
  const pending = generation;
  try {
    const page = await mygo.call("History.List", search.value, offset);
    if (generation !== pending) return;
    state = page.state;
    for (const row of page.rows) {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.textContent = "复制";
      button.addEventListener("click", async () => {
        try {
          await mygo.call("History.Copy", row.id, page.state);
          if (generation === pending) status.textContent = "已复制";
        } catch {
          if (generation === pending) await refresh();
        }
      });
      item.append(document.createTextNode((row.preview || row.kind) + " "), button);
      rows.append(item);
    }
    previous.disabled = offset === 0;
    next.disabled = !page.more;
    if (!page.rows.length) status.textContent = "没有匹配的记录";
  } catch {
    if (generation === pending) status.textContent = "无法读取历史记录";
  }
}

async function watch() {
  let cursor = 0;
  for (;;) {
    try {
      const update = await mygo.call("History.Changes", cursor);
      cursor = update.state.event_sequence;
      if (update.resync_required || update.events.length || (state &&
          (state.service_instance_id !== update.state.service_instance_id ||
           state.privacy_epoch !== update.state.privacy_epoch || state.history_revision !== update.state.history_revision))) {
        await refresh();
      }
    } catch {
      clearPage();
      status.textContent = "与本地服务的连接已中断，请重新打开窗口";
      return;
    }
  }
}

search.addEventListener("input", () => {
  offset = 0;
  clearPage();
  clearTimeout(timer);
  timer = setTimeout(refresh, 150);
});
previous.addEventListener("click", () => { offset = Math.max(0, offset - 40); refresh(); });
next.addEventListener("click", () => { offset += 40; refresh(); });
document.addEventListener("visibilitychange", refresh);
refresh();
watch();
