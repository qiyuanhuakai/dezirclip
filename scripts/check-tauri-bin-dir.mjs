import { readdirSync } from "node:fs";
import path from "node:path";

const rootDir = process.cwd();
const binDir = path.join(rootDir, "src-tauri", "src", "bin");

// tauri-cli 2.x 判定「要打包哪些二进制」时不查 cargo metadata，而是
// `read_dir(src/bin)` 并取每个目录项的 file_stem 当作二进制名；tauri-bundler
// 随后逐个从 `target/<profile>/<name>` 复制进安装包，缺文件直接让打包失败。
//
// 因此 `src/bin` 下的目录（以及任何非 .rs 条目）会被当成同名二进制，而 cargo
// 永远不会构建它。这个幽灵条目是否进入安装包还取决于 read_dir 的枚举顺序，
// 所以故障表现为「同一次发布只有部分 runner 失败」，极难定位。
//
// 规则：`src/bin` 只允许放真正的 bin 源文件。共享模块请放到 `src/<name>/`，
// 在 lib.rs 里用普通 `pub mod` 声明。调试工具用 `examples/`，它不参与打包。

const ALLOWED_EXTENSION = ".rs";

const run = async () => {
  let entries;
  try {
    entries = readdirSync(binDir, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") {
      console.log("✅ src-tauri/src/bin 不存在，无需检查");
      return;
    }
    throw error;
  }

  const offenders = [];
  for (const entry of entries) {
    if (!entry.isFile()) {
      offenders.push(`${entry.name}  (${entry.isDirectory() ? "目录" : "非普通文件"})`);
      continue;
    }
    if (path.extname(entry.name) !== ALLOWED_EXTENSION) {
      offenders.push(`${entry.name}  (非 ${ALLOWED_EXTENSION} 文件)`);
    }
  }

  console.log("src-tauri/src/bin 检查结果");
  console.log(`- 目录: ${binDir}`);
  console.log(`- 条目数: ${entries.length}`);
  console.log(`- 违规条目: ${offenders.length}`);

  if (offenders.length > 0) {
    console.log("\n[违规条目]");
    console.log(offenders.join("\n"));
    console.log(
      "\n这些条目会被 tauri-cli 当作待打包二进制，导致 deb/rpm/AppImage/NSIS 打包失败。\n" +
        "处理方式：模块目录移到 src/<name>/ 并在 lib.rs 里 `pub mod` 声明；" +
        "调试工具移到 examples/。"
    );
    process.exitCode = 1;
    return;
  }

  console.log("\n✅ src/bin 只含 bin 源文件");
};

run().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
