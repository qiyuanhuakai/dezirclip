import type { ConfirmDialogOptions, MessageDialogOptions, OpenDialogOptions, SaveDialogOptions } from "@tauri-apps/plugin-dialog";
import { shellCall } from "./bridge";

export async function open(options: OpenDialogOptions = {}) {
  const paths = await shellCall<string[] | null>("Open", options);
  return paths?.length ? (options.multiple ? paths : paths[0]) : null;
}

export async function save(options: SaveDialogOptions = {}) {
  return await shellCall<string>("Save", options) || null;
}

export async function ask(text: string, options: ConfirmDialogOptions | string = {}) {
  const opts = typeof options === "string" ? { title: options } : options;
  return (await shellCall<number>("Message", text, opts.title || "", opts.kind || "question",
    [opts.okLabel || "OK", opts.cancelLabel || "Cancel"], 1)) === 0;
}

export async function message(text: string, options: MessageDialogOptions | string = {}) {
  const opts = typeof options === "string" ? { title: options } : options;
  const configured = opts.buttons;
  const labels = typeof configured === "object" ?
    (configured.yes !== undefined ? [configured.yes, configured.no, configured.cancel] :
      configured.cancel !== undefined ? [configured.ok, configured.cancel] : [configured.ok]) :
    configured === "YesNoCancel" ? ["Yes", "No", "Cancel"] :
    configured === "YesNo" ? ["Yes", "No"] :
    configured === "OkCancel" ? ["Ok", "Cancel"] : [opts.okLabel || "Ok"];
  const choice = await shellCall<number>("Message", text, opts.title || "", opts.kind || "info", labels, labels.length - 1);
  return labels[choice] ?? "Cancel";
}
