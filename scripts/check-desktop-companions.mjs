import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1"], { encoding: "utf8" }));
for (const [name, owner] of [["dezirclip", "dezirclip"], ["dzc", "dzc-cli"], ["dzc-service", "dzc-service"]]) {
  const producers = metadata.packages.flatMap(pkg => pkg.targets.filter(target =>
    target.name === name && target.kind.includes("bin")).map(() => pkg.name));
  if (producers.length !== 1 || producers[0] !== owner) throw new Error(`Expected one ${name} binary from ${owner}`);
}
const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.sidecars.json", import.meta.url), "utf8"));
const external = config.bundle?.externalBin;
if (JSON.stringify(external) !== JSON.stringify(["binaries/dzc", "binaries/dzc-service"])) {
  throw new Error("The desktop bundle must register the CLI and Service once each");
}
