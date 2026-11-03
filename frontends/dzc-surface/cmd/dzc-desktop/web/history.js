"use strict";

const search = document.querySelector("#search");
const rows = document.querySelector("#rows");
const status = document.querySelector("#status");
document.querySelector("#close").addEventListener("click", () => mygo.window.close());
const previous = document.querySelector("#previous");
const next = document.querySelector("#next");
let offset = 0;
let generation = 0;
let state;
let subscribed = false;
let timer;
let previewTimer;
let previewSequence = 0;

function hidePreview() {
  clearTimeout(previewTimer);
  if (state) mygo.call("Preview.Select", ++previewSequence, 0, state).catch(() => {});
}

function clearPage() {
  hidePreview();
  generation++;
  state = undefined;
  rows.replaceChildren();
  previous.disabled = true;
  next.disabled = true;
  status.textContent = "";
}

async function refresh() {
  clearPage();
  if (document.hidden || !subscribed) return;
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
      const showPreview = () => {
        hidePreview();
        previewTimer = setTimeout(() => {
          if (generation === pending) mygo.call("Preview.Select", ++previewSequence, row.id, page.state).catch(() => {});
        }, 240);
      };
      item.addEventListener("pointerenter", showPreview);
      item.addEventListener("pointerleave", hidePreview);
      button.addEventListener("focus", showPreview);
      button.addEventListener("blur", hidePreview);
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
  let backoff = 250;
  for (;;) {
    try {
      const update = await mygo.call("History.Changes", cursor);
      const recovered = !subscribed;
      subscribed = true;
      backoff = 250;
      cursor = update.state.event_sequence;
      if (recovered || update.resync_required || update.events.length || (state &&
          (state.service_instance_id !== update.state.service_instance_id ||
           state.privacy_epoch !== update.state.privacy_epoch || state.history_revision !== update.state.history_revision))) {
        await refresh();
      }
    } catch {
      subscribed = false;
      clearPage();
      status.textContent = "与本地服务的连接已中断，正在重新连接";
      await new Promise(resolve => setTimeout(resolve, backoff));
      backoff = Math.min(backoff * 2, 4000);
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
window.addEventListener("scroll", hidePreview, true);
watch();
