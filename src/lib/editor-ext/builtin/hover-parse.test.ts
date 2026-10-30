import { describe, expect, it } from "vitest";
import { composeHoverContent, parseParameters, parseReturnValue } from "./hover-parse";

describe("parseParameters", () => {
  it("extracts TS params with types and @param docs", () => {
    const sig = "function add(a: number, b: number): number";
    const doc = "@param a first operand\n@param b second operand";
    expect(parseParameters(sig, doc, "typescript")).toEqual([
      { name: "a", type: "number", doc: "first operand" },
      { name: "b", type: "number", doc: "second operand" },
    ]);
  });

  it("extracts Rust params", () => {
    const sig = "fn add(a: i32, b: i32) -> i32";
    expect(parseParameters(sig, null, "rust")).toEqual([
      { name: "a", type: "i32", doc: null },
      { name: "b", type: "i32", doc: null },
    ]);
  });

  it("extracts Python params", () => {
    const sig = "def greet(name: str, age: int) -> str:";
    expect(parseParameters(sig, null, "python")).toEqual([
      { name: "name", type: "str", doc: null },
      { name: "age", type: "int", doc: null },
    ]);
  });

  it("extracts Go params (name type)", () => {
    const sig = "func add(a int, b int) int";
    expect(parseParameters(sig, null, "go")).toEqual([
      { name: "a", type: "int", doc: null },
      { name: "b", type: "int", doc: null },
    ]);
  });

  it("extracts Java params (type name)", () => {
    const sig = "void process(int count, String name)";
    expect(parseParameters(sig, null, "java")).toEqual([
      { name: "count", type: "int", doc: null },
      { name: "name", type: "String", doc: null },
    ]);
  });

  it("returns empty array for no-arg functions", () => {
    expect(parseParameters("fn main() {}", null, "rust")).toEqual([]);
    expect(parseParameters("function noop(): void", null, "typescript")).toEqual([]);
  });

  it("returns params without docs when @param absent", () => {
    const sig = "function foo(x: number): void";
    expect(parseParameters(sig, null, "typescript")).toEqual([
      { name: "x", type: "number", doc: null },
    ]);
  });
});

describe("parseReturnValue", () => {
  it("extracts TS return type and @returns doc", () => {
    const sig = "function add(a: number, b: number): number";
    const doc = "@returns the sum";
    expect(parseReturnValue(sig, doc, "typescript")).toEqual({ type: "number", doc: "the sum" });
  });

  it("extracts Rust return type", () => {
    expect(parseReturnValue("fn add(a: i32, b: i32) -> i32", null, "rust")).toEqual({ type: "i32", doc: null });
  });

  it("extracts Python return type", () => {
    expect(parseReturnValue("def greet(name: str) -> str:", null, "python")).toEqual({ type: "str", doc: null });
  });

  it("extracts Go return type", () => {
    expect(parseReturnValue("func add(a int, b int) int {", null, "go")).toEqual({ type: "int", doc: null });
  });

  it("returns null for void/no return", () => {
    expect(parseReturnValue("function noop(): void", null, "typescript")).toEqual({ type: "void", doc: null });
    expect(parseReturnValue("fn main() {}", null, "rust")).toBeNull();
  });

  it("returns type without doc when @returns absent", () => {
    expect(parseReturnValue("fn add(a: i32, b: i32) -> i32", null, "rust")).toEqual({ type: "i32", doc: null });
  });
});

describe("composeHoverContent", () => {
  it("class kind omits parameter section", () => {
    const contents = composeHoverContent({
      kind: "class",
      signature: "class Foo",
      doc: "A foo class",
      params: [{ name: "x", type: "int", doc: null }],
      returnValue: null,
      location: "src/foo.ts:1",
      container: null,
      refCount: 3,
      language: "typescript",
    });
    const joined = contents.map((c) => c.value).join("\n");
    expect(joined).toContain("class Foo");
    expect(joined).toContain("A foo class");
    expect(joined).not.toContain("Parameters");
    expect(joined).toContain("3 refs");
  });

  it("method kind shows params and returns", () => {
    const contents = composeHoverContent({
      kind: "method",
      signature: "function add(a: number, b: number): number",
      doc: null,
      params: [
        { name: "a", type: "number", doc: null },
        { name: "b", type: "number", doc: null },
      ],
      returnValue: { type: "number", doc: null },
      location: "src/math.ts:5",
      container: "MathUtil",
      refCount: 2,
      language: "typescript",
    });
    const joined = contents.map((c) => c.value).join("\n");
    expect(joined).toContain("Parameters");
    expect(joined).toContain("Returns");
    expect(joined).toContain("container: MathUtil");
  });

  it("property kind shows type/returns but not params", () => {
    const contents = composeHoverContent({
      kind: "property",
      signature: "name: string",
      doc: "The name property",
      params: [],
      returnValue: { type: "string", doc: null },
      location: null,
      container: "Foo",
      refCount: 0,
      language: "typescript",
    });
    const joined = contents.map((c) => c.value).join("\n");
    expect(joined).toContain("Returns");
    expect(joined).not.toContain("Parameters");
    expect(joined).not.toContain("---");
  });

  it("omits refs when refCount is zero", () => {
    const contents = composeHoverContent({
      kind: "function",
      signature: "function foo()",
      doc: null,
      params: [],
      returnValue: null,
      location: null,
      container: null,
      refCount: 0,
      language: "typescript",
    });
    const joined = contents.map((c) => c.value).join("\n");
    expect(joined).not.toContain("refs");
  });
});
