import path from "node:path";
import { fileURLToPath } from "node:url";
import { runTests } from "@vscode/test-electron";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const extensionDevelopmentPath = path.resolve(__dirname, "..");
const extensionTestsPath = path.resolve(__dirname, "suite", "index.mjs");
const workspacePath = path.resolve(__dirname, "fixtures");
const defaultVsCodeExe =
  process.platform === "win32"
    ? "C:/Users/user/AppData/Local/Programs/Microsoft VS Code/Code.exe"
    : undefined;
const vscodeExecutablePath = process.env.VSCODE_EXECUTABLE_PATH ?? defaultVsCodeExe;

async function main() {
  try {
    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs: [workspacePath, "--disable-extensions"],
      vscodeExecutablePath,
    });
  } catch (error) {
    console.error("[integration-test] VS Code extension-host tests failed.");
    console.error(error);
    process.exit(1);
  }
}

await main();
