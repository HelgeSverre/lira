//! Differential / metamorphic property fuzzer.
//!
//! Generates random *well-typed* Lira programs over a small all-`int` subset
//! (variables, a fixed array, a two-field struct, `+ - *`, plain and compound
//! assignment, `println`) and checks Lira's actual output against a trivial
//! Rust oracle that evaluates the same program. Any divergence is a real
//! compiler/VM bug — this is the class of silent-wrong-output bug that
//! crash-fuzzing (cargo-fuzz) cannot catch.
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
//! semantics match), and fully-parenthesized rendering (evaluation order
//! matches the oracle tree).

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
enum Expr {
    Lit(i64),
    Var(usize),
    Arr(usize),
    Field(usize),
    Bin(BinOp, Box<Expr>, Box<Expr>),
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

fn oracle(prog: &Program) -> Vec<i64> {
    let mut s = State {
        vars: [0; NVARS],
        arr: [0; NARR],
        fields: [0; NFIELDS],
    };
    let mut out = Vec::new();
    for stmt in &prog.stmts {
        match stmt {
            Stmt::Assign(lv, e) => {
                let v = eval(e, &s);
                match lv {
                    LVal::Var(i) => s.vars[*i] = v,
                    LVal::Arr(i) => s.arr[*i] = v,
                    LVal::Field(i) => s.fields[*i] = v,
                }
            }
            Stmt::Compound(lv, op, e) => {
                let rhs = eval(e, &s);
                let slot = match lv {
                    LVal::Var(i) => &mut s.vars[*i],
                    LVal::Arr(i) => &mut s.arr[*i],
                    LVal::Field(i) => &mut s.fields[*i],
                };
                *slot = op.eval(*slot, rhs);
            }
            Stmt::Print(e) => out.push(eval(e, &s)),
        }
    }
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

fn render_lval(lv: &LVal) -> String {
    match lv {
        LVal::Var(i) => format!("v{}", i),
        LVal::Arr(i) => format!("arr[{}]", i),
        LVal::Field(i) => format!("s.f{}", i),
    }
}

fn render(prog: &Program) -> String {
    let mut body = String::new();
    // Initialize all state to zero.
    for i in 0..NVARS {
        body.push_str(&format!("var v{} = 0\n", i));
    }
    body.push_str("var arr = [0, 0, 0, 0]\n");
    body.push_str("var s = S { f0: 0, f1: 0 }\n");
    for stmt in &prog.stmts {
        match stmt {
            Stmt::Assign(lv, e) => {
                body.push_str(&format!("{} = {}\n", render_lval(lv), render_expr(e)));
            }
            Stmt::Compound(lv, op, e) => {
                body.push_str(&format!(
                    "{} {}= {}\n",
                    render_lval(lv),
                    op.sym(),
                    render_expr(e)
                ));
            }
            Stmt::Print(e) => {
                body.push_str(&format!("println({})\n", render_expr(e)));
            }
        }
    }

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

fn lval() -> impl Strategy<Value = LVal> {
    prop_oneof![
        (0usize..NVARS).prop_map(LVal::Var),
        (0usize..NARR).prop_map(LVal::Arr),
        (0usize..NFIELDS).prop_map(LVal::Field),
    ]
}

fn stmt() -> impl Strategy<Value = Stmt> {
    prop_oneof![
        (lval(), expr()).prop_map(|(l, e)| Stmt::Assign(l, e)),
        (lval(), binop(), expr()).prop_map(|(l, o, e)| Stmt::Compound(l, o, e)),
        expr().prop_map(Stmt::Print),
    ]
}

fn program() -> impl Strategy<Value = Program> {
    (
        prop::collection::vec(stmt(), 1..12),
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
