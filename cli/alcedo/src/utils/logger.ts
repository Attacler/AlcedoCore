import chalk from "chalk";
import ora from "ora";

export function success(msg: string): void {
  console.log(chalk.green("✔"), msg);
}

export function error(msg: string): void {
  console.error(chalk.red("✖"), msg);
}

export function info(msg: string): void {
  console.log(chalk.blue("ℹ"), msg);
}

export function warn(msg: string): void {
  console.warn(chalk.yellow("⚠"), msg);
}

/**
 * Create an ora spinner with the given text.
 * Starts immediately. Call `.succeed()`, `.fail()`, or `.stop()` on the returned instance.
 * Ora auto-detects CI/non-TTY environments and disables spinners appropriately.
 */
export function createSpinner(text: string): ora.Ora {
  return ora({ text, color: "cyan" }).start();
}
