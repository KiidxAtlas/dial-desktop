// Formulas for interactive calculators in answers. A small arithmetic language parsed here, never
// JavaScript: numbers, named values, + - * / % ^, comparisons, && || !, `a ? b : c`, and a fixed
// set of math functions. Anything else is a parse error.

export type Formula =
  | { op: "num"; value: number }
  | { op: "var"; name: string }
  | { op: "neg" | "not"; arg: Formula }
  | { op: "bin"; sym: string; left: Formula; right: Formula }
  | { op: "if"; test: Formula; then: Formula; else: Formula }
  | { op: "call"; fn: string; args: Formula[] };

const FUNCTIONS: Record<string, { arity: [number, number]; run: (...a: number[]) => number }> = {
  min: { arity: [1, 16], run: Math.min },
  max: { arity: [1, 16], run: Math.max },
  abs: { arity: [1, 1], run: Math.abs },
  round: { arity: [1, 2], run: (x, d = 0) => { const f = 10 ** Math.round(d); return Math.round(x * f) / f; } },
  floor: { arity: [1, 1], run: Math.floor },
  ceil: { arity: [1, 1], run: Math.ceil },
  sqrt: { arity: [1, 1], run: Math.sqrt },
  exp: { arity: [1, 1], run: Math.exp },
  ln: { arity: [1, 1], run: Math.log },
  log: { arity: [1, 1], run: Math.log10 },
  log2: { arity: [1, 1], run: Math.log2 },
  pow: { arity: [2, 2], run: Math.pow },
  clamp: { arity: [3, 3], run: (x, lo, hi) => Math.min(Math.max(x, lo), hi) },
  if: { arity: [3, 3], run: (c, a, b) => (c ? a : b) },
};

const MAX_LENGTH = 400;
const MAX_DEPTH = 40;
const TOKEN = /\s*(?:(\d+(?:\.\d*)?(?:e[+-]?\d+)?|\.\d+(?:e[+-]?\d+)?)|([A-Za-z_]\w*)|(<=|>=|==|!=|&&|\|\||[-+*/%^()<>?:,!]))/y;

/** Parse `source`, or throw an `Error` saying what is wrong. */
export function parseFormula(source: string): Formula {
  if (source.length > MAX_LENGTH) throw new Error("formula is too long");
  const tokens: string[] = [];
  TOKEN.lastIndex = 0;
  while (TOKEN.lastIndex < source.length) {
    const start = TOKEN.lastIndex;
    const m = TOKEN.exec(source);
    if (!m) {
      if (!source.slice(start).trim()) break;
      throw new Error(`unexpected "${source.slice(start).trim()[0]}"`);
    }
    tokens.push(m[1] ?? m[2] ?? m[3]);
  }
  let pos = 0;
  let depth = 0;
  const peek = () => tokens[pos];
  const take = (t?: string) => {
    const tok = tokens[pos];
    if (tok === undefined || (t !== undefined && tok !== t)) throw new Error(tok === undefined ? `expected ${t ?? "a value"}` : `unexpected "${tok}"`);
    pos++;
    return tok;
  };
  const binary = (next: () => Formula, ops: string[]) => () => {
    let left = next();
    while (ops.includes(peek())) {
      const sym = take();
      left = { op: "bin", sym, left, right: next() } as Formula;
    }
    return left;
  };
  function ternary(): Formula {
    if (++depth > MAX_DEPTH) throw new Error("formula is nested too deeply");
    const test = or();
    let node = test;
    if (peek() === "?") {
      take("?");
      const then = ternary();
      take(":");
      node = { op: "if", test, then, else: ternary() };
    }
    depth--;
    return node;
  }
  const unary = (): Formula => {
    if (peek() === "-" || peek() === "!") {
      const sym = take();
      if (++depth > MAX_DEPTH) throw new Error("formula is nested too deeply");
      const arg = unary();
      depth--;
      return { op: sym === "-" ? "neg" : "not", arg };
    }
    if (peek() === "+") {
      take();
      if (++depth > MAX_DEPTH) throw new Error("formula is nested too deeply");
      const arg = unary();
      depth--;
      return arg;
    }
    return power();
  };
  // Right-associative, and binds tighter than a leading minus on its left: -2^2 is -4.
  const power = (): Formula => {
    const base = atom();
    if (peek() !== "^") return base;
    take("^");
    if (++depth > MAX_DEPTH) throw new Error("formula is nested too deeply");
    const right = unary();
    depth--;
    return { op: "bin", sym: "^", left: base, right };
  };
  const product = binary(unary, ["*", "/", "%"]);
  const sum = binary(product, ["+", "-"]);
  const compare = binary(sum, ["<", "<=", ">", ">=", "==", "!="]);
  const and = binary(compare, ["&&"]);
  const or = binary(and, ["||"]);
  function atom(): Formula {
    const tok = take();
    if (tok === "(") { const inner = ternary(); take(")"); return inner; }
    if (/^[\d.]/.test(tok)) return { op: "num", value: Number(tok) };
    if (/^[A-Za-z_]/.test(tok)) {
      if (peek() !== "(") return { op: "var", name: tok };
      if (!Object.hasOwn(FUNCTIONS, tok)) throw new Error(`unknown function "${tok}"`);
      const fn = FUNCTIONS[tok];
      take("(");
      const args: Formula[] = [];
      if (peek() !== ")") {
        args.push(ternary());
        while (peek() === ",") { take(","); args.push(ternary()); }
      }
      take(")");
      if (args.length < fn.arity[0] || args.length > fn.arity[1]) throw new Error(`${tok}() takes ${fn.arity[0] === fn.arity[1] ? fn.arity[0] : `${fn.arity[0]}–${fn.arity[1]}`} values`);
      return { op: "call", fn: tok, args };
    }
    throw new Error(`unexpected "${tok}"`);
  }
  if (!tokens.length) throw new Error("formula is empty");
  const tree = ternary();
  if (pos < tokens.length) throw new Error(`unexpected "${tokens[pos]}"`);
  return tree;
}

/** The names a formula reads, so a calculator can reject unknown ones up front. */
export function formulaNames(f: Formula, out = new Set<string>()): Set<string> {
  switch (f.op) {
    case "var": out.add(f.name); break;
    case "neg": case "not": formulaNames(f.arg, out); break;
    case "bin": formulaNames(f.left, out); formulaNames(f.right, out); break;
    case "if": formulaNames(f.test, out); formulaNames(f.then, out); formulaNames(f.else, out); break;
    case "call": for (const a of f.args) formulaNames(a, out); break;
  }
  return out;
}

/** Evaluate with named values; unknown names and invalid math give NaN. */
export function evaluate(f: Formula, values: Record<string, number>): number {
  switch (f.op) {
    case "num": return f.value;
    case "var": return Object.hasOwn(values, f.name) ? values[f.name] : NaN;
    case "neg": return -evaluate(f.arg, values);
    case "not": return evaluate(f.arg, values) ? 0 : 1;
    case "if": return evaluate(f.test, values) ? evaluate(f.then, values) : evaluate(f.else, values);
    case "call": return Object.hasOwn(FUNCTIONS, f.fn) ? FUNCTIONS[f.fn].run(...f.args.map(a => evaluate(a, values))) : NaN;
    case "bin": {
      const a = evaluate(f.left, values);
      if (f.sym === "&&") return a ? (evaluate(f.right, values) ? 1 : 0) : 0;
      if (f.sym === "||") return a ? 1 : (evaluate(f.right, values) ? 1 : 0);
      const b = evaluate(f.right, values);
      switch (f.sym) {
        case "+": return a + b;
        case "-": return a - b;
        case "*": return a * b;
        case "/": return a / b;
        case "%": return a % b;
        case "^": return a ** b;
        case "<": return +(a < b);
        case "<=": return +(a <= b);
        case ">": return +(a > b);
        case ">=": return +(a >= b);
        case "==": return +(a === b);
        case "!=": return +(a !== b);
      }
      return NaN;
    }
  }
}
