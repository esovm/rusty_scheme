use parser;
use parser::Node;

use std::fmt;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

pub fn interpret(nodes: &Vec<Node>) -> Result<Value, RuntimeError> {
    let env = Environment::new_root();
    evaluate_nodes(nodes, env)
}

#[deriving(PartialEq, Clone)]
pub enum Value {
    Symbol(String),
    Integer(int),
    Boolean(bool),
    String(String),
    List(Vec<Value>),
    Procedure(Function),
}

// null == empty list
macro_rules! null { () => (List(vec![])) }

pub enum Function {
    NativeFunction(ValueOperation),
    SchemeFunction(Vec<String>, Vec<Node>),
}

// type signature for all native functions
type ValueOperation = fn(&[Node], Rc<RefCell<Environment>>) -> Result<Value, RuntimeError>;

impl fmt::Show for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl Value {
    fn to_str(&self) -> String {
        match self {
            &Symbol(_) => format!("'{}", self.to_raw_str()),
            &List(_) => format!("'{}", self.to_raw_str()),
            _ => self.to_raw_str()
        }
    }

    fn to_raw_str(&self) -> String {
        match *self {
            Symbol(ref val) => format!("{}", val),
            Integer(val) => format!("{}", val),
            Boolean(val) => format!("#{}", if val { "t" } else { "f" }),
            String(ref val) => format!("\"{}\"", val),
            List(ref val) => {
                let mut s = String::new();
                let mut first = true;
                for n in val.iter() {
                    if first {
                        first = false;
                    } else {
                        s = s.append(" ");
                    }
                    s = s.append(n.to_raw_str().as_slice());
                }
                format!("({})", s)
            }
            Procedure(_) => format!("#<procedure>")
        }
    }
}

impl PartialEq for Function {
    fn eq(&self, other: &Function) -> bool {
        self == other
    }
}

impl Clone for Function {
    fn clone(&self) -> Function {
        match *self {
            NativeFunction(ref func) => NativeFunction(*func),
            SchemeFunction(ref a, ref b) => SchemeFunction(a.clone(), b.clone())
        }
    }
}

pub struct RuntimeError {
    message: String,
}

impl fmt::Show for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RuntimeError: {}", self.message)
    }
}

macro_rules! runtime_error(
    ($($arg:tt)*) => (
        return Err(RuntimeError { message: format!($($arg)*)})
    )
)

struct Environment {
    parent: Option<Rc<RefCell<Environment>>>,
    values: HashMap<String, Value>,
}

impl Environment {
    fn new_root() -> Rc<RefCell<Environment>> {
        let mut env = Environment { parent: None, values: HashMap::new() };
        for item in PREDEFINED_FUNCTIONS.iter() {
            let (name, ref func) = *item;
            env.set(name.to_str(), Procedure(func.clone()));
        }
        Rc::new(RefCell::new(env))
    }

    fn new_child(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Environment>> {
        let env = Environment { parent: Some(parent), values: HashMap::new() };
        Rc::new(RefCell::new(env))
    }

    fn set(&mut self, key: String, value: Value) {
        self.values.insert(key, value);
    }

    fn has(&self, key: &String) -> bool {
        self.values.contains_key(key)
    }

    fn get(&self, key: &String) -> Option<Value> {
        match self.values.find(key) {
            Some(val) => Some(val.clone()),
            None => {
                // recurse up the environment tree until a value is found or the end is reached
                match self.parent {
                    Some(ref parent) => parent.borrow().get(key),
                    None => None
                }
            }
        }
    }
}

fn evaluate_nodes(nodes: &Vec<Node>, env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    let mut result = null!();
    for node in nodes.iter() {
        result = try!(evaluate_node(node, env.clone()));
    };
    Ok(result)
}

fn evaluate_node(node: &Node, env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    match node {
        &parser::Identifier(ref v) => {
            match env.borrow().get(v) {
                Some(val) => Ok(val),
                None => runtime_error!("Identifier not found: {}", node)
            }
        },
        &parser::Integer(v) => Ok(Integer(v)),
        &parser::Boolean(v) => Ok(Boolean(v)),
        &parser::String(ref v) => Ok(String(v.clone())),
        &parser::List(ref vec) => {
            if vec.len() > 0 {
                evaluate_expression(vec, env.clone())
            } else {
                Ok(null!())
            }
        }
    }
}

fn quote_node(node: &Node, quasi: bool, env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    match node {
        &parser::Identifier(ref v) => Ok(Symbol(v.clone())),
        &parser::Integer(v) => Ok(Integer(v)),
        &parser::Boolean(v) => Ok(Boolean(v)),
        &parser::String(ref v) => Ok(String(v.clone())),
        &parser::List(ref vec) => {
            // check if we are unquoting inside a quasiquote
            if quasi && vec.len() > 0 && *vec.get(0) == parser::Identifier("unquote".to_str()) {
                if vec.len() != 2 {
                    runtime_error!("Must supply exactly one argument to unquote: {}", vec);
                }
                evaluate_node(vec.get(1), env.clone())
            } else {
                let mut res = vec![];
                for n in vec.iter() {
                    let v = try!(quote_node(n, quasi, env.clone()));
                    res.push(v);
                }
                Ok(List(res))
            }
        }
    }
}

fn evaluate_expression(nodes: &Vec<Node>, env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if nodes.len() == 0 {
        runtime_error!("Can't evaluate an empty expression: {}", nodes);
    }
    let first = try!(evaluate_node(nodes.get(0), env.clone()));
    match first {
        Procedure(f) => apply_function(&f, nodes.tailn(1), env.clone()),
        _ => runtime_error!("First element in an expression must be a procedure: {}", first)
    }
}

fn apply_function(func: &Function, args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    match func {
        &NativeFunction(nativeFn) => {
            nativeFn(args, env)
        },
        &SchemeFunction(ref argNames, ref body) => {
            if argNames.len() != args.len() {
                runtime_error!("Must supply exactly {} arguments to function: {}", argNames.len(), args);
            }

            // create a new, child environment for the procedure and define the arguments as local variables
            let procEnv = Environment::new_child(env.clone());
            for (name, arg) in argNames.iter().zip(args.iter()) {
                let val = try!(evaluate_node(arg, env.clone()));
                procEnv.borrow_mut().set(name.clone(), val);
            }

            Ok(try!(evaluate_nodes(body, procEnv)))
        }
    }
}

static PREDEFINED_FUNCTIONS: &'static[(&'static str, Function)] = &[
    ("define", NativeFunction(native_define)),
    ("set!", NativeFunction(native_set)),
    ("lambda", NativeFunction(native_lambda)),
    ("λ", NativeFunction(native_lambda)),
    ("if", NativeFunction(native_if)),
    ("+", NativeFunction(native_plus)),
    ("-", NativeFunction(native_minus)),
    ("and", NativeFunction(native_and)),
    ("or", NativeFunction(native_or)),
    ("list", NativeFunction(native_list)),
    ("quote", NativeFunction(native_quote)),
    ("quasiquote", NativeFunction(native_quasiquote)),
    ("error", NativeFunction(native_error)),
];

fn native_define(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        runtime_error!("Must supply exactly two arguments to define: {}", args);
    }
    let name = match *args.get(0).unwrap() {
        parser::Identifier(ref x) => x,
        _ => runtime_error!("Unexpected node for name in define: {}", args)
    };
    let alreadyDefined = env.borrow().has(name);
    if !alreadyDefined {
        let val = try!(evaluate_node(args.get(1).unwrap(), env.clone()));
        env.borrow_mut().set(name.clone(), val);
        Ok(null!())
    } else {
        runtime_error!("Duplicate define: {}", name)
    }
}

fn native_set(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        runtime_error!("Must supply exactly two arguments to set!: {}", args);
    }
    let name = match *args.get(0).unwrap() {
        parser::Identifier(ref x) => x,
        _ => runtime_error!("Unexpected node for name in set!: {}", args)
    };
    let alreadyDefined = env.borrow().has(name);
    if alreadyDefined {
        let val = try!(evaluate_node(args.get(1).unwrap(), env.clone()));
        env.borrow_mut().set(name.clone(), val);
        Ok(null!())
    } else {
        runtime_error!("Can't set! an undefined variable: {}", name)
    }
}

fn native_lambda(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        runtime_error!("Must supply at least two arguments to lambda: {}", args);
    }
    let argNames = match *args.get(0).unwrap() {
        parser::List(ref list) => {
            let mut names = vec![];
            for item in list.iter() {
                match *item {
                    parser::Identifier(ref s) => names.push(s.clone()),
                    _ => runtime_error!("Unexpected argument in lambda arguments: {}", item)
                };
            }
            names
        }
        _ => runtime_error!("Unexpected node for arguments in lambda: {}", args)
    };
    let body = Vec::from_slice(args.tailn(1));
    Ok(Procedure(SchemeFunction(argNames, body)))
}

fn native_if(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 3 {
        runtime_error!("Must supply exactly three arguments to if: {}", args);
    }
    let condition = try!(evaluate_node(args.get(0).unwrap(), env.clone()));
    match condition {
        Boolean(false) => evaluate_node(args.get(2).unwrap(), env.clone()),
        _ => evaluate_node(args.get(1).unwrap(), env.clone())
    }
}

fn native_plus(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() < 2 {
        runtime_error!("Must supply at least two arguments to +: {}", args);
    }
    let mut sum = 0;
    for n in args.iter() {
        let v = try!(evaluate_node(n, env.clone()));
        match v {
            Integer(x) => sum += x,
            _ => runtime_error!("Unexpected node during +: {}", n)
        };
    };
    Ok(Integer(sum))
}

fn native_minus(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 2 {
        runtime_error!("Must supply exactly two arguments to -: {}", args);
    }
    let l = try!(evaluate_node(args.get(0).unwrap(), env.clone()));
    let r = try!(evaluate_node(args.get(1).unwrap(), env.clone()));
    let mut result = match l {
        Integer(x) => x,
        _ => runtime_error!("Unexpected node during -: {}", args)
    };
    result -= match r {
        Integer(x) => x,
        _ => runtime_error!("Unexpected node during -: {}", args)
    };
    Ok(Integer(result))
}

fn native_and(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    let mut res = Boolean(true);
    for n in args.iter() {
        let v = try!(evaluate_node(n, env.clone()));
        match v {
            Boolean(false) => return Ok(Boolean(false)),
            _ => res = v
        }
    }
    Ok(res)
}

fn native_or(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    for n in args.iter() {
        let v = try!(evaluate_node(n, env.clone()));
        match v {
            Boolean(false) => (),
            _ => return Ok(v)
        }
    }
    Ok(Boolean(false))
}

fn native_list(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    let mut elements = vec![];
    for n in args.iter() {
        let v = try!(evaluate_node(n, env.clone()));
        elements.push(v);
    }
    Ok(List(elements))
}

fn native_quote(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        runtime_error!("Must supply exactly one argument to quote: {}", args);
    }
    quote_node(args.get(0).unwrap(), false, env.clone())
}

fn native_quasiquote(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        runtime_error!("Must supply exactly one argument to quasiquote: {}", args);
    }
    quote_node(args.get(0).unwrap(), true, env.clone())
}

fn native_error(args: &[Node], env: Rc<RefCell<Environment>>) -> Result<Value, RuntimeError> {
    if args.len() != 1 {
        runtime_error!("Must supply exactly one arguments to error: {}", args);
    }
    let e = try!(evaluate_node(args.get(0).unwrap(), env.clone()));
    runtime_error!("{}", e);
}

#[test]
fn test_global_variables() {
    assert_eq!(interpret(&vec![parser::List(vec![parser::Identifier("define".to_str()), parser::Identifier("x".to_str()), parser::Integer(2)]), parser::List(vec![parser::Identifier("+".to_str()), parser::Identifier("x".to_str()), parser::Identifier("x".to_str()), parser::Identifier("x".to_str())])]).unwrap(),
               Integer(6));
}

#[test]
fn test_global_function_definition() {
    assert_eq!(interpret(&vec![parser::List(vec![parser::Identifier("define".to_str()), parser::Identifier("double".to_str()), parser::List(vec![parser::Identifier("lambda".to_str()), parser::List(vec![parser::Identifier("x".to_str())]), parser::List(vec![parser::Identifier("+".to_str()), parser::Identifier("x".to_str()), parser::Identifier("x".to_str())])])]), parser::List(vec![parser::Identifier("double".to_str()), parser::Integer(8)])]).unwrap(),
               Integer(16));
}
