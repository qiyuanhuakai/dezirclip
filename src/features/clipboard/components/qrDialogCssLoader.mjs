import fs from "node:fs";
const qrDialogCss = fs.readFileSync(
  new URL(import.meta.resolve("@deziro/ui-react/styles/components/QrCodeDialog.css")),
  "utf-8"
);
export default qrDialogCss;
