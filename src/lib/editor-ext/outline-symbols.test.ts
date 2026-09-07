import { describe, expect, it } from "vitest";
import { extractSymbols, regexSymbolProvider } from "./builtin/outline-symbols";

describe("outline-symbols — regex extraction per language", () => {
  it("extracts ts functions/classes/interfaces/types and arrow functions", () => {
    const src = [
      "export function alpha() {}",
      "  async function beta() {}",
      "export class Gamma {}",
      "export interface Delta {",
      "export type Epsilon = string;",
      "const zeta = (a: number) => a;",
      "const eta = async (a: number) => a;",
      "const theta = outer();",
    ].join("\n");
    const syms = extractSymbols("typescript", src);
    expect(syms.map((s) => `${s.kind}:${s.name}`)).toEqual([
      "function:alpha",
      "function:beta",
      "class:Gamma",
      "interface:Delta",
      "type:Epsilon",
      "function:zeta",
      "function:eta",
    ]);
  });

  it("extracts js functions and classes", () => {
    const src = "function one() {}\nmodule.exports = function two() {}\nclass Two {}";
    const syms = extractSymbols("javascript", src);
    expect(syms.map((s) => s.name)).toEqual(["one", "Two"]);
  });

  it("extracts rust fns/structs/enums/traits/impls with pub modifiers", () => {
    const src = [
      "pub fn serve() {}",
      "    pub(crate) async fn run() {}",
      "pub struct Nuomi;",
      "enum Mode {",
      "pub trait Kernel {",
      "impl Kernel for Nuomi {",
      "impl Default for Nuomi {",
    ].join("\n");
    const syms = extractSymbols("rust", src);
    expect(syms.map((s) => `${s.kind}:${s.name}`)).toEqual([
      "function:serve",
      "function:run",
      "struct:Nuomi",
      "enum:Mode",
      "trait:Kernel",
      "impl:Nuomi",
      "impl:Nuomi",
    ]);
  });

  it("extracts python defs and classes", () => {
    const src = "def main():\n    pass\n\nasync def fetch():\n    pass\n\nclass Store:\n    pass";
    const syms = extractSymbols("python", src);
    expect(syms.map((s) => `${s.kind}:${s.name}`)).toEqual([
      "function:main",
      "function:fetch",
      "class:Store",
    ]);
  });

  it("extracts markdown headings with level detail", () => {
    const src = "# Title\n\ntext\n\n## Section A\n### Sub 1";
    const syms = extractSymbols("markdown", src);
    expect(syms.map((s) => `${s.kind}:${s.name}:${s.detail}`)).toEqual([
      "heading:Title:h1",
      "heading:Section A:h2",
      "heading:Sub 1:h3",
    ]);
  });

  it("uses the fallback rules for unknown languages and reports 0-based LSP lines", () => {
    const src = "def legacy():\n    pass";
    const syms = extractSymbols("someLang", src);
    expect(syms).toHaveLength(1);
    expect(syms[0]?.name).toBe("legacy");
    expect(syms[0]?.range.start.line).toBe(0);
    expect(syms[0]?.range.start.character).toBe(0);
  });

  it("the registered provider is language-agnostic and delegates to extractSymbols", () => {
    const provider = regexSymbolProvider();
    expect(provider.languages).toEqual(["*"]);
    const syms = provider.provideSymbols({
      path: "src/main.rs",
      language: "rust",
      content: "fn main() {}",
    });
    expect(syms.map((s) => s.name)).toEqual(["main"]);
  });
});
