import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Blend, ChevronDown, ChevronLeft, ChevronRight, CircleHelp, Clock, File, FileArchive, FileChartColumn,
  FileCode, FileSpreadsheet, FileText, Github, GripHorizontal, Image, MessageSquare, Minus, Music,
  Pin, PinOff, Plus, RotateCcw, Scan, Search, Settings, Smile, Tag, Trash2, Video, X } from "lucide-react";

const directory = new URL("../packages/ui-native/uikit/icons/", import.meta.url);
const symbols = { Blend, ChevronDown, ChevronLeft, ChevronRight, CircleHelp, Clock, File, FileArchive, FileChartColumn,
  FileCode, FileSpreadsheet, FileText, Github, GripHorizontal, Image, MessageSquare, Minus, Music,
  Pin, PinOff, Plus, RotateCcw, Scan, Search, Settings, Smile, Tag, Trash2, Video, X };
for (const [name, symbol] of Object.entries(symbols)) {
  const path = fileURLToPath(new URL(name + ".svg", directory));
  const svg = renderToStaticMarkup(createElement(symbol)) + "\n";
  if (process.argv.includes("--check")) {
    if (await readFile(path, "utf8") !== svg) throw Error("Native icon differs from the Web icon: " + name);
  } else {
    await writeFile(path, svg, "utf8");
  }
}
