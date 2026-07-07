import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const qrDialogCss = fs.readFileSync(
  path.resolve(here, "../../../styles/components/QrCodeDialog.css"),
  "utf-8"
);
export default qrDialogCss;