//! Differential / metamorphic property fuzzer.
//!
//! Generates random *well-typed* Lira programs over a small all-`int` subset
//! (variables, a fixed array, a two-field struct, `+ - *`, comparisons and
//! booleans, `if`/`else`, bounded `while`, plain and compound assignment,
//! `println`) and checks Lira's actual output against a trivial Rust oracle
//! that evaluates the same program. Any divergence is a real compiler/VM bug —
//! the silent-wrong-output class that crash-fuzzing cannot catch.
//!
//! It would have caught both bugs found by hand:
//!   * `main()` double-invocation — each program is rendered three ways
//!     (top-level; wrapped in `fn main` + explicit call; wrapped relying on
//!     auto-invoke) and all three must equal the single oracle output.
//!   * field/element assignment no-op — the subset leans heavily on mutating
//!     `arr[i]` and `s.fN`, which the oracle tracks exactly.
//!
//! Divergence sources are engineered out so a failure means a bug, not a
//! subset gap: no division/modulo (no div-by-zero), only in-range literal
//! indices (no out-of-bounds), small literals + wrapping arithmetic (integer
//! semantics match), fully-parenthesized rendering (evaluation order matches
//! the oracle tree), and `while` loops run a fixed number of iterations via a
//! hidden counter (guaranteed termination on both sides).

use proptest::prelude::*;

const NVARS: usize = 3;
const NARR: usize = 4;
const NFIELDS: usize = 2;

#[derive(Clone, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
}

impl BinOp {
    fn eval(&self, a: i64, b: i64) -> i64 {
        match self {
            BinOp::Add => a.wrapping_add(b),
            BinOp::Sub => a.wrapping_sub(b),
            BinOp::Mul => a.wrapping_mul(b),
        }
    }
    fn sym(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
        }
    }
}

#[derive(Clone, Debug)]
enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    fn eval(&self, a: i64, b: i64) -> bool {
        match self {
            CmpOp::Lt => a < b,
            CmpOp::Gt => a > b,
            CmpOp::Le => a <= b,
            CmpOp::Ge => a >= b,
            CmpOp::Eq => a == b,
            CmpOp::Ne => a != b,
        }
    }
    fn sym(&self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Gt => ">",
            CmpOp::Le => "<=",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Lit(i64),
    Var(usize),
    Arr(usize),
    Field(usize),
    Bin(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
enum Bool {
    Lit(bool),
    Cmp(CmpOp, Expr, Expr),
    And(Box<Bool>, Box<Bool>),
    Or(Box<Bool>, Box<Bool>),
    Not(Box<Bool>),
}

#[derive(Clone, Debug)]
enum LVal {
    Var(usize),
    Arr(usize),
    Field(usize),
}

#[derive(Clone, Debug)]
enum Stmt {
    Assign(LVal, Expr),
    Compound(LVal, BinOp, Expr),
    Print(Expr),
    If(Bool, Vec<Stmt>, Vec<Stmt>),
    While(u32, Vec<Stmt>),
}

#[derive(Clone, Debug)]
struct Program {
    stmts: Vec<Stmt>,
    wrap_in_main: bool,
    explicit_call: bool,
}

// ---------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------

struct State {
    vars: [i64; NVARS],
    arr: [i64; NARR],
    fields: [i64; NFIELDS],
}

fn eval(e: &Expr, s: &State) -> i64 {
    match e {
        Expr::Lit(n) => *n,
        Expr::Var(i) => s.vars[*i],
        Expr::Arr(i) => s.arr[*i],
        Expr::Field(i) => s.fields[*i],
        Expr::Bin(op, a, b) => op.eval(eval(a, s), eval(b, s)),
    }
}

fn eval_bool(b: &Bool, s: &State) -> bool {
    match b {
        Bool::Lit(v) => *v,
        Bool::Cmp(op, a, b) => op.eval(eval(a, s), eval(b, s)),
        Bool::And(a, b) => eval_bool(a, s) && eval_bool(b, s),
        Bool::Or(a, b) => eval_bool(a, s) || eval_bool(b, s),
        Bool::Not(a) => !eval_bool(a, s),
    }
}

fn exec(stmts: &[Stmt], s: &mut State, out: &mut Vec<i64>) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign(lv, e) => {
                let v = eval(e, s);
                match lv {
                    LVal::Var(i) => s.vars[*i] = v,
                    LVal::Arr(i) => s.arr[*i] = v,
                    LVal::Field(i) => s.fields[*i] = v,
                }
            }
            Stmt::Compound(lv, op, e) => {
                let rhs = eval(e, s);
                let slot = match lv {
                    LVal::Var(i) => &mut s.vars[*i],
                    LVal::Arr(i) => &mut s.arr[*i],
                    LVal::Field(i) => &mut s.fields[*i],
                };
                *slot = op.eval(*slot, rhs);
            }
            Stmt::Print(e) => out.push(eval(e, s)),
            Stmt::If(cond, then_b, else_b) => {
                if eval_bool(cond, s) {
                    exec(then_b, s, out);
                } else {
                    exec(else_b, s, out);
                }
            }
            Stmt::While(bound, body) => {
                for _ in 0..*bound {
                    exec(body, s, out);
                }
            }
        }
    }
}

fn oracle(prog: &Program) -> Vec<i64> {
    let mut s = State {
        vars: [0; NVARS],
        arr: [0; NARR],
        fields: [0; NFIELDS],
    };
    let mut out = Vec::new();
    exec(&prog.stmts, &mut s, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Render to Lira source (fully parenthesized so precedence matches the oracle)
// ---------------------------------------------------------------------------

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Lit(n) => {
            if *n < 0 {
                format!("({})", n)
            } else {
                n.to_string()
            }
        }
        Expr::Var(i) => format!("v{}", i),
        Expr::Arr(i) => format!("arr[{}]", i),
        Expr::Field(i) => format!("s.f{}", i),
        Expr::Bin(op, a, b) => {
            format!("({} {} {})", render_expr(a), op.sym(), render_expr(b))
        }
    }
}

fn render_bool(b: &Bool) -> String {
    match b {
        Bool::Lit(v) => v.to_string(),
        Bool::Cmp(op, a, b) => format!("({} {} {})", render_expr(a), op.sym(), render_expr(b)),
        Bool::And(a, b) => format!("({} && {})", render_bool(a), render_bool(b)),
        Bool::Or(a, b) => format!("({} || {})", render_bool(a), render_bool(b)),
        Bool::Not(a) => format!("(!{})", render_bool(a)),
    }
}

fn render_lval(lv: &LVal) -> String {
    match lv {
        LVal::Var(i) => format!("v{}", i),
        LVal::Arr(i) => format!("arr[{}]", i),
        LVal::Field(i) => format!("s.f{}", i),
    }
}

/// Render a statement list. `loop_id` hands out unique hidden loop-counter
/// names so nested `while`s don't collide.
fn render_stmts(stmts: &[Stmt], out: &mut String, indent: usize, loop_id: &mut usize) {
    let pad = "    ".repeat(indent);
    for stmt in stmts {
        match stmt {
            Stmt::Assign(lv, e) => {
                out.push_str(&format!("{}{} = {}\n", pad, render_lval(lv), render_expr(e)));
            }
            Stmt::Compound(lv, op, e) => {
                out.push_str(&format!(
                    "{}{} {}= {}\n",
                    pad,
                    render_lval(lv),
                    op.sym(),
                    render_expr(e)
                ));
            }
            Stmt::Print(e) => {
                out.push_str(&format!("{}println({})\n", pad, render_expr(e)));
            }
            Stmt::If(cond, then_b, else_b) => {
                out.push_str(&format!("{}if {} {{\n", pad, render_bool(cond)));
                render_stmts(then_b, out, indent + 1, loop_id);
                out.push_str(&format!("{}}} else {{\n", pad));
                render_stmts(else_b, out, indent + 1, loop_id);
                out.push_str(&format!("{}}}\n", pad));
            }
            Stmt::While(bound, body) => {
                // Bounded loop via a fresh hidden counter the body can't touch.
                let id = *loop_id;
                *loop_id += 1;
                out.push_str(&format!("{}var __i{} = 0\n", pad, id));
                out.push_str(&format!("{}while __i{} < {} {{\n", pad, id, bound));
                render_stmts(body, out, indent + 1, loop_id);
                out.push_str(&format!("{}    __i{} = __i{} + 1\n", pad, id, id));
                out.push_str(&format!("{}}}\n", pad));
            }
        }
    }
}

fn render(prog: &Program) -> String {
    let mut body = String::new();
    let indent = if prog.wrap_in_main { 1 } else { 0 };
    let pad = "    ".repeat(indent);
    for i in 0..NVARS {
        body.push_str(&format!("{}var v{} = 0\n", pad, i));
    }
    body.push_str(&format!("{}var arr = [0, 0, 0, 0]\n", pad));
    body.push_str(&format!("{}var s = S {{ f0: 0, f1: 0 }}\n", pad));
    let mut loop_id = 0usize;
    render_stmts(&prog.stmts, &mut body, indent, &mut loop_id);

    let mut src = String::from("struct S { f0: int, f1: int }\n");
    if prog.wrap_in_main {
        src.push_str("fn main() {\n");
        src.push_str(&body);
        src.push_str("}\n");
        if prog.explicit_call {
            src.push_str("main()\n");
        }
    } else {
        src.push_str(&body);
    }
    src
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn binop() -> impl Strategy<Value = BinOp> {
    prop_oneof![Just(BinOp::Add), Just(BinOp::Sub), Just(BinOp::Mul)]
}

fn cmpop() -> impl Strategy<Value = CmpOp> {
    prop_oneof![
        Just(CmpOp::Lt),
        Just(CmpOp::Gt),
        Just(CmpOp::Le),
        Just(CmpOp::Ge),
        Just(CmpOp::Eq),
        Just(CmpOp::Ne),
    ]
}

fn expr() -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![
        (-20i64..20).prop_map(Expr::Lit),
        (0usize..NVARS).prop_map(Expr::Var),
        (0usize..NARR).prop_map(Expr::Arr),
        (0usize..NFIELDS).prop_map(Expr::Field),
    ];
    leaf.prop_recursive(3, 12, 2, |inner| {
        (binop(), inner.clone(), inner)
            .prop_map(|(op, a, b)| Expr::Bin(op, Box::new(a), Box::new(b)))
    })
}

fn bool_expr() -> impl Strategy<Value = Bool> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Bool::Lit),
        (cmpop(), expr(), expr()).prop_map(|(op, a, b)| Bool::Cmp(op, a, b)),
    ];
    leaf.prop_recursive(2, 8, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone())
                .prop_map(|(a, b)| Bool::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Bool::Or(Box::new(a), Box::new(b))),
            inner.prop_map(|a| Bool::Not(Box::new(a))),
        ]
    })
}

fn lval() -> impl Strategy<Value = LVal> {
    prop_oneof![
        (0usize..NVARS).prop_map(LVal::Var),
        (0usize..NARR).prop_map(LVal::Arr),
        (0usize..NFIELDS).prop_map(LVal::Field),
    ]
}

fn stmt() -> impl Strategy<Value = Stmt> {
    let leaf = prop_oneof![
        (lval(), expr()).prop_map(|(l, e)| Stmt::Assign(l, e)),
        (lval(), binop(), expr()).prop_map(|(l, o, e)| Stmt::Compound(l, o, e)),
        expr().prop_map(Stmt::Print),
    ];
    leaf.prop_recursive(3, 40, 3, |inner| {
        prop_oneof![
            (
                bool_expr(),
                prop::collection::vec(inner.clone(), 1..4),
                prop::collection::vec(inner.clone(), 1..4),
            )
                .prop_map(|(c, t, e)| Stmt::If(c, t, e)),
            (1u32..5, prop::collection::vec(inner, 1..4)).prop_map(|(n, b)| Stmt::While(n, b)),
        ]
    })
}

fn program() -> impl Strategy<Value = Program> {
    (
        prop::collection::vec(stmt(), 1..10),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(stmts, wrap_in_main, explicit_call)| Program {
            stmts,
            wrap_in_main,
            explicit_call,
        })
}

fn run_lira(src: &str) -> Result<Vec<i64>, String> {
    let bytecode = lirac::compile_with_imports("fuzz.li", src)?;
    let (_code, output) = liravm::run_with_capture(&bytecode)?;
    output
        .iter()
        .map(|line| {
            line.trim()
                .parse::<i64>()
                .map_err(|_| format!("non-integer output line: {:?}", line))
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1500,
        // Integration tests have no crate root for the regression file; the
        // full failing source is printed on failure, so persistence is noise.
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn lira_matches_oracle(prog in program()) {
        let expected = oracle(&prog);
        let src = render(&prog);
        match run_lira(&src) {
            Ok(actual) => prop_assert_eq!(
                actual, expected,
                "\n--- program (wrap_in_main={}, explicit_call={}) ---\n{}",
                prog.wrap_in_main, prog.explicit_call, src
            ),
            Err(e) => prop_assert!(
                false,
                "program in the well-typed subset failed to compile/run: {}\n--- source ---\n{}",
                e, src
            ),
        }
    }
}
