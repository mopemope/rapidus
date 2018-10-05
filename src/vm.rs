use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};
use std::ffi::CString;
use std::rc::Rc;

use libc;
// use cpuprofiler::PROFILER;

use builtin;
use bytecode_gen::{ByteCode, VMInst};
// use jit::TracingJit;
use node::BinOp;

pub type RawStringPtr = *mut libc::c_char;

pub unsafe fn alloc_rawstring(s: &str) -> RawStringPtr {
    let p = libc::calloc(1, s.len() + 2) as RawStringPtr;
    libc::strncpy(p, s.as_ptr() as *const i8, s.len());
    p
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrayValue {
    pub elems: Vec<Value>,
    pub length: usize,
    pub obj: HashMap<String, Value>,
}

impl ArrayValue {
    pub fn new(arr: Vec<Value>) -> ArrayValue {
        let len = arr.len();
        ArrayValue {
            elems: arr,
            length: len,
            obj: {
                let mut hm = HashMap::new();
                hm.insert(
                    "__proto__".to_string(),
                    Value::Object(Rc::new(RefCell::new({
                        let mut hm = HashMap::new();
                        hm.insert(
                            "push".to_string(),
                            Value::NeedThis(Box::new(Value::BuiltinFunction(builtin::ARRAY_PUSH))),
                        );
                        hm
                    }))),
                );
                hm
            },
        }
    }
}

pub type CallObjectRef = Rc<RefCell<CallObject>>;

#[derive(Clone, Debug, PartialEq)]
pub struct CallObject {
    pub vals: Rc<RefCell<HashMap<String, Value>>>,
    pub param_names: Vec<String>,
    // TODO: Add 'arguments' object
    pub this: Option<Rc<RefCell<HashMap<String, Value>>>>,
    pub parent: Option<CallObjectRef>,
}

impl CallObject {
    pub fn new(this: Option<Rc<RefCell<HashMap<String, Value>>>>) -> CallObject {
        CallObject {
            vals: Rc::new(RefCell::new(HashMap::new())),
            param_names: vec![],
            this: this,
            parent: None,
        }
    }

    pub fn new_global() -> CallObjectRef {
        let vals = Rc::new(RefCell::new(HashMap::new()));
        let mut callobj = Rc::new(RefCell::new(CallObject {
            vals: vals.clone(),
            param_names: vec![],
            this: None,
            parent: None,
        }));
        callobj.borrow_mut().this = Some(vals);
        callobj
    }

    pub fn set_value(&mut self, name: String, val: Value) {
        self.vals.borrow_mut().insert(name, val);
    }

    pub fn set_value_if_exist(&mut self, name: String, val: Value) {
        match self.vals.borrow_mut().entry(name.clone()) {
            Entry::Occupied(ref mut v) => *v.get_mut() = val,
            Entry::Vacant(v) => {
                match self.parent {
                    Some(ref parent) => return parent.borrow_mut().set_value_if_exist(name, val),
                    None => v.insert(val),
                };
            }
        }
    }

    pub fn get_value(&self, name: &String) -> Value {
        if let Some(val) = self.vals.borrow().get(name) {
            return val.clone();
        }
        match self.parent {
            Some(ref parent) => return parent.borrow().get_value(name),
            None => panic!(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Bool(bool),
    Number(f64),
    String(CString),
    Function(usize, Rc<RefCell<HashMap<String, Value>>>, CallObject),
    NeedThis(Box<Value>),
    WithThis(Box<(Value, Value)>),               // Function, This
    BuiltinFunction(usize), // unknown if usize == 0; specific function if usize > 0
    Object(Rc<RefCell<HashMap<String, Value>>>), // Object(HashMap<String, Value>),
    Array(Rc<RefCell<ArrayValue>>),
    Arguments,
}

impl Value {
    pub fn to_string(self) -> String {
        match self {
            Value::String(name) => name.into_string().unwrap(),
            Value::Number(n) => format!("{}", n),
            e => unimplemented!("{:?}", e),
        }
    }
}

pub fn new_value_function(pos: usize, callobj: CallObject) -> Value {
    let mut val = Value::Function(
        pos,
        Rc::new(RefCell::new({
            let mut hm = HashMap::new();
            hm.insert(
                "prototype".to_string(),
                Value::Object(Rc::new(RefCell::new({
                    let mut hm = HashMap::new();
                    // hm.insert("call".to_string(), Value::NeedThis(Box::new(Value::BuiltinFunction(6))));
                    hm
                }))),
            );
            hm.insert(
                "__proto__".to_string(),
                Value::Object(Rc::new(RefCell::new({
                    let mut hm = HashMap::new();
                    hm.insert(
                        "call".to_string(),
                        Value::NeedThis(Box::new(Value::BuiltinFunction(
                            builtin::FUNCTION_PROTOTYPE_CALL,
                        ))),
                    );
                    hm
                }))),
            );
            hm
        })),
        callobj,
    );

    let v2 = val.clone();
    if let Value::Function(_, ref mut obj, _) = &mut val {
        // TODO: Add constructor of this function itself (==Function). (not prototype.constructor)
        if let Value::Object(ref mut obj) = (*obj.borrow_mut()).get_mut("prototype").unwrap() {
            obj.borrow_mut().insert("constructor".to_string(), v2);
        }
    }
    val
}

#[derive(Debug, Clone)]
pub struct ConstantTable {
    pub value: Vec<Value>,
    pub string: Vec<String>,
}

impl ConstantTable {
    pub fn new() -> ConstantTable {
        ConstantTable {
            value: vec![],
            string: vec![],
        }
    }
}

pub struct VM {
    // pub jit: TracingJit,
    pub state: VMState,
    pub const_table: ConstantTable,
    pub insts: ByteCode,
    pub loop_bgn_end: HashMap<isize, isize>,
    pub op_table: [fn(&mut VM); 49],
    pub builtin_functions: [unsafe fn(Vec<Value>, &mut VM); 7],
}

pub struct VMState {
    pub stack: Vec<Value>,
    pub scope: Vec<CallObjectRef>,
    pub bp: usize,
    pub lp: usize,
    pub pc: isize,
    pub history: Vec<(usize, usize, usize, isize)>, // bp, lp, sp, return_pc
}

impl VM {
    pub fn new(global_vals: CallObjectRef) -> VM {
        global_vals.borrow_mut().set_value("console".to_string(), {
            let mut map = HashMap::new();
            map.insert(
                "log".to_string(),
                Value::BuiltinFunction(builtin::CONSOLE_LOG),
            );
            Value::Object(Rc::new(RefCell::new(map)))
        });

        global_vals.borrow_mut().set_value("process".to_string(), {
            let mut map = HashMap::new();
            map.insert("stdout".to_string(), {
                let mut map = HashMap::new();
                map.insert(
                    "write".to_string(),
                    Value::BuiltinFunction(builtin::PROCESS_STDOUT_WRITE),
                );
                Value::Object(Rc::new(RefCell::new(map)))
            });
            Value::Object(Rc::new(RefCell::new(map)))
        });

        global_vals.borrow_mut().set_value("Math".to_string(), {
            let mut map = HashMap::new();
            map.insert(
                "floor".to_string(),
                Value::BuiltinFunction(builtin::MATH_FLOOR),
            );
            map.insert(
                "random".to_string(),
                Value::BuiltinFunction(builtin::MATH_RANDOM),
            );
            map.insert("pow".to_string(), Value::BuiltinFunction(builtin::MATH_POW));
            Value::Object(Rc::new(RefCell::new(map)))
        });

        VM {
            // jit: unsafe { TracingJit::new() },
            state: VMState {
                stack: { Vec::with_capacity(128) },
                scope: vec![global_vals],
                history: {
                    let mut s = Vec::with_capacity(128);
                    s.push((0, 0, 0, 0));
                    s
                },
                bp: 0,
                lp: 0,
                pc: 0isize,
            },
            const_table: ConstantTable::new(),
            insts: vec![],
            loop_bgn_end: HashMap::new(),
            op_table: [
                end,
                create_context,
                construct,
                create_object,
                create_array,
                push_int8,
                push_int32,
                push_false,
                push_true,
                push_const,
                push_this,
                push_arguments,
                neg,
                add,
                sub,
                mul,
                div,
                rem,
                lt,
                gt,
                le,
                ge,
                eq,
                ne,
                seq,
                sne,
                and,
                or,
                get_member,
                set_member,
                get_global,
                set_global,
                get_local,
                set_local,
                get_arg_local,
                set_arg_local,
                jmp_if_false,
                jmp,
                call,
                return_,
                assign_func_rest_param,
                double,
                pop,
                land,
                lor,
                set_cur_callobj,
                get_name,
                set_name,
                decl_var,
            ],
            builtin_functions: [
                builtin::console_log,
                builtin::process_stdout_write,
                builtin::array_push,
                builtin::math_floor,
                builtin::math_random,
                builtin::math_pow,
                builtin::function_prototype_call,
            ],
        }
    }
}

impl VM {
    pub fn run(&mut self, insts: ByteCode) {
        self.insts = insts;
        // Unlock the mutex and start the profiler
        // PROFILER
        //     .lock()
        //     .unwrap()
        //     .start("./my-prof.profile")
        //     .expect("Couldn't start");

        self.do_run();

        // Unwrap the mutex and stop the profiler
        // PROFILER.lock().unwrap().stop().expect("Couldn't stop");
    }

    pub fn do_run(&mut self) {
        loop {
            // if let Some(end) = self.loop_bgn_end.get(&self.state.pc) {
            //     unsafe {
            //         // println!("range: [{:x}, {:x})", self.state.pc, end);
            //         if let Some(pc) = self.jit.can_loop_jit(
            //             &self.insts,
            //             &self.const_table,
            //             &mut self.state,
            //             *end as usize,
            //         ) {
            //             self.state.pc = pc;
            //             continue;
            //         }
            //     }
            // }
            let code = self.insts[self.state.pc as usize];
            self.op_table[code as usize](self);
            if code == VMInst::RETURN || code == VMInst::END {
                break;
            }
            // println!("stack trace: {:?} - {}", self.stack, *pc);
        }
    }
}

macro_rules! get_int8 {
    ($self:ident, $var:ident, $ty:ty) => {
        let $var = $self.insts[$self.state.pc as usize] as $ty;
        $self.state.pc += 1;
    };
}

macro_rules! get_int32 {
    ($self:ident, $var:ident, $ty:ty) => {
        let $var = (($self.insts[$self.state.pc as usize + 3] as $ty) << 24)
            + (($self.insts[$self.state.pc as usize + 2] as $ty) << 16)
            + (($self.insts[$self.state.pc as usize + 1] as $ty) << 8)
            + ($self.insts[$self.state.pc as usize + 0] as $ty);
        $self.state.pc += 4;
    };
}

fn end(_self: &mut VM) {}

fn create_context(self_: &mut VM) {
    self_.state.pc += 1; // create_context
    get_int32!(self_, num_local_var, usize);
}

fn construct(self_: &mut VM) {
    self_.state.pc += 1; // construct
    get_int32!(self_, argc, usize);

    let mut callee = self_.state.stack.pop().unwrap();

    loop {
        match callee {
            Value::Function(dst, obj, mut callobj) => {
                self_.state.history.push((0, 0, 0, self_.state.pc));

                // insert new 'this'
                let pos = self_.state.stack.len() - argc;
                let new_this = {
                    let mut map = HashMap::new();
                    map.insert(
                        "__proto__".to_string(),
                        (*obj)
                            .borrow()
                            .get("prototype")
                            .unwrap_or(&Value::Undefined)
                            .clone(),
                    );
                    Rc::new(RefCell::new(map))
                };

                let mut args = vec![];
                for _ in 0..argc {
                    args.push(self_.state.stack.pop().unwrap());
                }
                for (i, arg) in args.iter().rev().enumerate() {
                    let param_name = callobj.param_names[i].clone();
                    callobj.set_value(param_name, arg.clone());
                }

                callobj.this = Some(new_this.clone());
                self_.state.scope.push(Rc::new(RefCell::new(callobj)));
                self_.state.pc = dst as isize;

                self_.do_run();

                self_.state.scope.pop();

                match self_.state.stack.last_mut().unwrap() {
                    &mut Value::Object(_)
                    | &mut Value::Array(_)
                    | &mut Value::Function(_, _, _)
                    | &mut Value::BuiltinFunction(_) => {}
                    others => *others = Value::Object(new_this),
                };
                break;
            }
            Value::NeedThis(callee_) => {
                callee = *callee_;
            }
            Value::WithThis(box (callee_, _)) => {
                callee = callee_;
            }
            c => {
                println!("Constract: err: {:?}, pc = {}", c, self_.state.pc);
                break;
            }
        }
    }
}

fn create_object(self_: &mut VM) {
    self_.state.pc += 1; // create_context
    get_int32!(self_, len, usize);

    let mut map = HashMap::new();
    for _ in 0..len {
        let name = if let Value::String(name) = self_.state.stack.pop().unwrap() {
            name.into_string().unwrap()
        } else {
            panic!()
        };
        let val = self_.state.stack.pop().unwrap();
        map.insert(name, val.clone());
    }
    self_
        .state
        .stack
        .push(Value::Object(Rc::new(RefCell::new(map))));
}

fn create_array(self_: &mut VM) {
    self_.state.pc += 1; // create_context
    get_int32!(self_, len, usize);

    let mut arr = vec![];
    for _ in 0..len {
        let val = self_.state.stack.pop().unwrap();
        arr.push(val);
    }

    self_
        .state
        .stack
        .push(Value::Array(Rc::new(RefCell::new(ArrayValue::new(arr)))));
}

fn push_int8(self_: &mut VM) {
    self_.state.pc += 1; // push_int
    get_int8!(self_, n, i32);
    self_.state.stack.push(Value::Number(n as f64));
}

fn push_int32(self_: &mut VM) {
    self_.state.pc += 1; // push_int
    get_int32!(self_, n, i32);
    self_.state.stack.push(Value::Number(n as f64));
}

fn push_false(self_: &mut VM) {
    self_.state.pc += 1; // push_false
    self_.state.stack.push(Value::Bool(false));
}

fn push_true(self_: &mut VM) {
    self_.state.pc += 1; // push_true
    self_.state.stack.push(Value::Bool(true));
}

fn push_const(self_: &mut VM) {
    self_.state.pc += 1; // push_const
    get_int32!(self_, n, usize);
    self_.state.stack.push(self_.const_table.value[n].clone());
}

fn push_this(self_: &mut VM) {
    self_.state.pc += 1; // push_this
    let val = self_.state.stack[self_.state.bp].clone();
    self_.state.stack.push(val);
}

fn push_arguments(self_: &mut VM) {
    self_.state.pc += 1; // push_arguments
    self_.state.stack.push(Value::Arguments);
}

fn neg(self_: &mut VM) {
    self_.state.pc += 1; // neg
    let expr = self_.state.stack.last_mut().unwrap();
    match expr {
        &mut Value::Number(ref mut n) => *n = -*n,
        _ => unimplemented!(),
    }
}

macro_rules! bin_op {
    ($name:ident, $binop:ident) => {
        fn $name(self_: &mut VM) {
            self_.state.pc += 1; // $name
            binary(self_, &BinOp::$binop);
        }
    };
}

bin_op!(add, Add);
bin_op!(sub, Sub);
bin_op!(mul, Mul);
bin_op!(div, Div);
bin_op!(rem, Rem);
bin_op!(lt, Lt);
bin_op!(gt, Gt);
bin_op!(le, Le);
bin_op!(ge, Ge);
bin_op!(eq, Eq);
bin_op!(ne, Ne);
bin_op!(seq, SEq);
bin_op!(sne, SNe);
bin_op!(and, And);
bin_op!(or, Or);

#[inline]
fn binary(self_: &mut VM, op: &BinOp) {
    let rhs = self_.state.stack.pop().unwrap();
    let lhs = self_.state.stack.pop().unwrap();
    match (lhs, rhs) {
        (Value::Number(n1), Value::Number(n2)) => self_.state.stack.push(match op {
            &BinOp::Add => Value::Number(n1 + n2),
            &BinOp::Sub => Value::Number(n1 - n2),
            &BinOp::Mul => Value::Number(n1 * n2),
            &BinOp::Div => Value::Number(n1 / n2),
            &BinOp::Rem => Value::Number((n1 as i64 % n2 as i64) as f64),
            &BinOp::Lt => Value::Bool(n1 < n2),
            &BinOp::Gt => Value::Bool(n1 > n2),
            &BinOp::Le => Value::Bool(n1 <= n2),
            &BinOp::Ge => Value::Bool(n1 >= n2),
            &BinOp::Eq => Value::Bool(n1 == n2),
            &BinOp::Ne => Value::Bool(n1 != n2),
            &BinOp::SEq => Value::Bool(n1 == n2),
            &BinOp::SNe => Value::Bool(n1 != n2),
            &BinOp::And => Value::Number(((n1 as i64) & (n2 as i64)) as f64),
            &BinOp::Or => Value::Number(((n1 as i64) | (n2 as i64)) as f64),
            _ => panic!(),
        }),
        (Value::String(s1), Value::Number(n2)) => self_.state.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", s1.to_str().unwrap(), n2);
                Value::String(CString::new(concat).unwrap())
            }
            _ => panic!(),
        }),
        (Value::Number(n1), Value::String(s2)) => self_.state.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", n1, s2.to_str().unwrap());
                Value::String(CString::new(concat).unwrap())
            }
            _ => panic!(),
        }),
        (Value::String(s1), Value::String(s2)) => self_.state.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", s1.to_str().unwrap(), s2.to_str().unwrap());
                Value::String(CString::new(concat).unwrap())
            }
            _ => panic!(),
        }),
        _ => {}
    }
}

fn get_member(self_: &mut VM) {
    self_.state.pc += 1; // get_global
    let member = self_.state.stack.pop().unwrap();
    let parent = self_.state.stack.pop().unwrap();
    match parent.clone() {
        Value::String(s) => {
            match member {
                // Index
                Value::Number(n) if n - n.floor() == 0.0 => self_.state.stack.push(Value::String(
                    CString::new(
                        s.to_str()
                            .unwrap()
                            .chars()
                            .nth(n as usize)
                            .unwrap()
                            .to_string(),
                    ).unwrap(),
                )),
                Value::String(ref member) if member.to_str().unwrap() == "length" => {
                    self_.state.stack.push(Value::Number(
                        s.to_str()
                            .unwrap()
                            .chars()
                            .fold(0, |x, c| x + c.len_utf16()) as f64,
                    ));
                }
                // TODO: Support all features.
                _ => self_.state.stack.push(Value::Undefined),
            }
        }
        Value::Object(map) => {
            match obj_find_val(&map.borrow().clone(), member.to_string().as_str()) {
                Value::Function(pos, map2, mut callobj) => {
                    self_.state.stack.push(Value::Function(pos, map2, {
                        callobj.this = Some(map.clone());
                        callobj
                    }))
                }
                val => self_.state.stack.push(val),
            }
        }
        Value::Function(pos, map, _) | Value::NeedThis(box Value::Function(pos, map, _)) => {
            match obj_find_val(&map.borrow().clone(), member.to_string().as_str()) {
                Value::Function(pos, map2, mut callobj) => {
                    self_.state.stack.push(Value::Function(pos, map2, {
                        callobj.this = Some(map.clone());
                        callobj
                    }))
                }
                val => self_.state.stack.push(val),
            }
        }
        Value::Array(map) => {
            let mut map = map.borrow_mut();
            match member {
                // Index
                Value::Number(n) if n - n.floor() == 0.0 => {
                    let arr = &map.elems;
                    if n as usize >= map.length {
                        self_.state.stack.push(Value::Undefined);
                    } else {
                        self_.state.stack.push(arr[n as usize].clone())
                    }
                }
                Value::String(ref s) if s.to_str().unwrap() == "length" => {
                    self_.state.stack.push(Value::Number(map.length as f64));
                }
                _ => match obj_find_val(&map.obj, member.to_string().as_str()) {
                    Value::NeedThis(callee) => self_
                        .state
                        .stack
                        .push(Value::WithThis(Box::new((*callee, parent)))),
                    val => self_.state.stack.push(val),
                },
            }
        }
        Value::Arguments => {
            match member {
                // Index
                Value::Number(n) if n - n.floor() == 0.0 => {
                    let idx = self_.state.bp + n as usize;
                    if idx < self_.state.lp {
                        let val = self_.state.stack[idx].clone();
                        self_.state.stack.push(val);
                    }
                }
                Value::String(ref s) if s.to_str().unwrap() == "length" => {
                    self_
                        .state
                        .stack
                        .push(Value::Number(self_.state.lp as f64 - self_.state.bp as f64));
                }
                _ => self_.state.stack.push(Value::Undefined),
            }
        }
        e => unreachable!("{:?}", e),
    }
}

pub fn obj_find_val(obj: &HashMap<String, Value>, key: &str) -> Value {
    match obj.get(key) {
        Some(addr) => addr.clone(),
        None => match obj.get("__proto__") {
            Some(Value::Object(obj)) => obj_find_val(&*(*obj).borrow(), key),
            _ => Value::Undefined,
        },
    }
}

fn set_member(self_: &mut VM) {
    self_.state.pc += 1; // get_global
    let member = self_.state.stack.pop().unwrap();
    let parent = self_.state.stack.pop().unwrap();
    let val = self_.state.stack.pop().unwrap();
    match parent {
        Value::Object(map)
        | Value::Function(_, map, _)
        | Value::NeedThis(box Value::Function(_, map, _)) => {
            *map.borrow_mut()
                .entry(member.to_string())
                .or_insert_with(|| Value::Undefined) = val;
        }
        Value::Array(map) => {
            let mut map = map.borrow_mut();
            match member {
                // Index
                Value::Number(n) if n - n.floor() == 0.0 => {
                    if n as usize >= map.length as usize {
                        map.length = n as usize;
                        unsafe {
                            map.elems.set_len(n as usize);
                        };
                    }
                    map.elems[n as usize] = val;
                }
                Value::String(ref s) if s.to_str().unwrap() == "length" => match val {
                    Value::Number(n) if n - n.floor() == 0.0 => map.length = n as usize,
                    _ => {}
                },
                _ => {
                    *map.obj
                        .entry(member.to_string())
                        .or_insert_with(|| Value::Undefined) = val
                }
            }
        }
        Value::Arguments => {
            match member {
                // Index
                Value::Number(n) if n - n.floor() == 0.0 => {
                    let idx = self_.state.bp + n as usize;
                    if idx < self_.state.lp {
                        self_.state.stack[idx] = val;
                    }
                }
                _ => {}
            }
        }
        e => unreachable!("{:?}", e),
    }
}

fn get_global(self_: &mut VM) {
    self_.state.pc += 1; // get_global
    get_int32!(self_, n, usize);
    // let val = (*(*self_.global_objects)
    //     .borrow()
    //     .get(self_.const_table.string[n].as_str())
    //     .unwrap())
    //     .clone();
    // self_.state.stack.push(val);
}

fn set_global(self_: &mut VM) {
    self_.state.pc += 1; // set_global
    get_int32!(self_, n, usize);
    // *(*self_.global_objects)
    //     .borrow_mut()
    //     .entry(self_.const_table.string[n].clone())
    //     .or_insert_with(|| Value::Undefined) = self_.state.stack.pop().unwrap();
}

fn get_local(self_: &mut VM) {
    self_.state.pc += 1; // get_local
    get_int32!(self_, n, usize);
    let val = self_.state.stack[self_.state.lp + n].clone();
    self_.state.stack.push(val);
}

fn set_local(self_: &mut VM) {
    self_.state.pc += 1; // set_local
    get_int32!(self_, n, usize);
    let val = self_.state.stack.pop().unwrap();
    self_.state.stack[self_.state.lp + n] = val;
}

fn get_arg_local(self_: &mut VM) {
    self_.state.pc += 1; // get_arg_local
    get_int32!(self_, n, usize);
    let val = self_.state.stack[self_.state.bp + n].clone();
    self_.state.stack.push(val);
}

fn set_arg_local(self_: &mut VM) {
    self_.state.pc += 1; // set_arg_local
    get_int32!(self_, n, usize);
    let val = self_.state.stack.pop().unwrap();
    self_.state.stack[self_.state.bp + n] = val;
}

fn jmp(self_: &mut VM) {
    self_.state.pc += 1; // jmp
    get_int32!(self_, dst, i32);
    if dst < 0 {
        self_
            .loop_bgn_end
            .insert(self_.state.pc + dst as isize, self_.state.pc);
    }
    self_.state.pc += dst as isize;
}

fn jmp_if_false(self_: &mut VM) {
    self_.state.pc += 1; // jmp_if_false
    get_int32!(self_, dst, i32);
    let cond = self_.state.stack.pop().unwrap();
    if let Value::Bool(false) = cond {
        self_.state.pc += dst as isize
    }
}

fn call(self_: &mut VM) {
    self_.state.pc += 1; // Call
    get_int32!(self_, argc, usize);
    let mut argc = argc;

    let mut this = None;

    let mut callee = self_.state.stack.pop().unwrap();

    loop {
        match callee {
            Value::BuiltinFunction(x) => {
                let mut args = vec![];
                for _ in 0..argc {
                    args.push(self_.state.stack.pop().unwrap());
                }
                args.reverse();
                if let Some(this) = this {
                    args.insert(0, this)
                }
                unsafe { self_.builtin_functions[x](args, self_) };
                break;
            }
            Value::Function(dst, _, mut callobj) => {
                if args_all_number(&self_.state.stack, argc) {
                    // if let Some(f) = unsafe {
                    //     self_
                    //         .jit
                    //         .can_jit(&self_.insts, &self_.const_table, dst, argc)
                    // } {
                    //     let mut args = vec![];
                    //     for _ in 0..argc {
                    //         args.push(self_.state.stack.pop().unwrap());
                    //     }
                    //     args.reverse();
                    //     self_
                    //         .state
                    //         .stack
                    //         .push(unsafe { self_.jit.run_llvm_func(dst, f, args) });
                    //     break;
                    // }
                }

                callobj.vals = Rc::new(RefCell::new(HashMap::new()));

                let mut args = vec![];
                for _ in 0..argc {
                    args.push(self_.state.stack.pop().unwrap());
                }
                for (i, arg) in args.iter().rev().enumerate() {
                    let param_name = callobj.param_names[i].clone();
                    callobj.set_value(param_name, arg.clone());
                }

                self_.state.scope.push(Rc::new(RefCell::new(callobj)));
                self_
                    .state
                    .history
                    .push((0, 0, self_.state.stack.len(), self_.state.pc));
                self_.state.pc = dst as isize;

                self_.do_run();

                self_.state.scope.pop();

                // self_
                //     .jit
                //     .register_return_type(dst, self_.state.stack.last().unwrap());
                break;
            }
            Value::NeedThis(callee_) => {
                // this = Some(Value::Object(self_.global_objects.clone()));
                callee = *callee_;
            }
            Value::WithThis(box callee_this) => {
                this = Some(callee_this.1);
                callee = callee_this.0;
            }
            c => {
                println!("Call: err: {:?}, pc = {}", c, self_.state.pc);
                break;
            }
        }
    }

    fn args_all_number(stack: &Vec<Value>, argc: usize) -> bool {
        let stack_len = stack.len();
        stack[stack_len - argc..stack_len].iter().all(|v| match v {
            &Value::Number(_) => true,
            _ => false,
        })
    }
}

fn return_(self_: &mut VM) {
    let len = self_.state.stack.len();
    // println!("s: {:?}", self_.state.stack);
    if let Some((bp, lp, sp, return_pc)) = self_.state.history.pop() {
        self_.state.stack.drain(sp..len - 1);
        self_.state.pc = return_pc;
    } else {
        unreachable!()
    }
    // println!("a: {:?}", self_.state.stack);
}

fn assign_func_rest_param(self_: &mut VM) {
    self_.state.pc += 1; // assign_func_rest_param
    get_int32!(self_, num_func_param, usize);
    get_int32!(self_, dst_var_id, usize);
    let mut rest_params = vec![];
    for i in num_func_param..(self_.state.lp - self_.state.bp) {
        rest_params.push(self_.state.stack[self_.state.bp + i].clone());
    }
    self_.state.stack[self_.state.lp + dst_var_id] =
        Value::Array(Rc::new(RefCell::new(ArrayValue::new(rest_params))));
}

fn double(self_: &mut VM) {
    self_.state.pc += 1; // double
    let stack_top_val = self_.state.stack.last().unwrap().clone();
    self_.state.stack.push(stack_top_val);
}

fn pop(self_: &mut VM) {
    self_.state.pc += 1; // double
    self_.state.stack.pop();
}

// land & lor are for JIT compiler. They don't make sense in VM.

fn land(self_: &mut VM) {
    self_.state.pc += 1; // land
}

fn lor(self_: &mut VM) {
    self_.state.pc += 1; // lor
}

fn set_cur_callobj(self_: &mut VM) {
    self_.state.pc += 1;
    if let Some(Value::Function(_, _, ref mut callobj)) = self_.state.stack.last_mut() {
        callobj.parent = Some(self_.state.scope.last().unwrap().clone());
    }
}

fn get_name(self_: &mut VM) {
    self_.state.pc += 1;
    get_int32!(self_, name_id, usize);
    let name = &self_.const_table.string[name_id];
    let val = self_.state.scope.last().unwrap().borrow().get_value(name);
    self_.state.stack.push(val);
}

fn set_name(self_: &mut VM) {
    self_.state.pc += 1;
    get_int32!(self_, name_id, usize);
    let name = self_.const_table.string[name_id].clone();
    let val = self_.state.stack.pop().unwrap();
    self_
        .state
        .scope
        .last()
        .unwrap()
        .borrow_mut()
        .set_value_if_exist(name, val);
}

fn decl_var(self_: &mut VM) {
    self_.state.pc += 1;
    get_int32!(self_, name_id, usize);
    let name = self_.const_table.string[name_id].clone();
    let val = self_.state.stack.pop().unwrap();
    self_
        .state
        .scope
        .last()
        .unwrap()
        .borrow_mut()
        .set_value(name, val);
}

// #[rustfmt::skip]
// pub fn vm2_test() {
//     let mut vm2 = VM::new();
//     vm2.const_table.value.push(Value::Function(41, Rc::new(RefCell::new(HashMap::new()))));
//     vm2.const_table.value.push(Value::String("log".to_string()));
//     vm2.const_table.string.push("console".to_string());
//
//     // Loop for 100,000,000
//     // AllocLocalVar(1, 1)
//     // Push(Number(0.0))
//     // SetLocal(1)
//     // GetLocal(1)
//     // Push(Number(100000000.0))
//     // Lt
//     // JmpIfFalse(6)
//     // GetLocal(1)
//     // Push(Number(1.0))
//     // Add
//     // SetLocal(1)
//     // Jmp(-8)
//     // End
//     // vm2.run(vec![
//     //         CREATE_CONTEXT, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 1, 1
//     //         PUSH_INT32, 0x00, 0x00, 0x00, 0x00, // PushInt 0
//     //         SET_LOCAL, 0x01, 0x00, 0x00, 0x00, // SetLocal 1
//     //         GET_LOCAL, 0x01, 0x00, 0x00, 0x00, // GetLocal 1
//     //         PUSH_INT32, 0x00, 0xe1, 0xf5, 0x05, // PushInt 100,000,000
//     //         LT, // Lt
//     //         JMP_IF_FALSE, 0x15, 0x00, 0x00, 0x00, // JmpIfFalse 21
//     //         GET_LOCAL, 0x01, 0x00, 0x00, 0x00, // GetLocal 1
//     //         PUSH_INT32, 0x01, 0x00, 0x00, 0x00, // PushInt 1
//     //         ADD, // Add
//     //         SET_LOCAL, 0x01, 0x00, 0x00, 0x00, // SetLocal 1
//     //         JMP, 0xdb, 0xff, 0xff, 0xff, // Jmp -37
//     //         END, // End
//     // ]);
//
//     // Fibo 10
//     // AllocLocalVar(0, 1)
//     // Push(Number(10.0))
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // End
//     // AllocLocalVar(0, 1)
//     // GetLocal(0)
//     // Push(Number(2.0))
//     // Lt
//     // JmpIfFalse(3)
//     // Push(Number(1.0))
//     // Return
//     // GetLocal(0)
//     // Push(Number(1.0))
//     // Sub
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // GetLocal(0)
//     // Push(Number(2.0))
//     // Sub
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // Add
//     // Return
//     vm2.run(vec![
//         CREATE_CONTEXT, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 1, 1
//         PUSH_INT32, 35,0,0,0, // PushInt 10
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         GET_GLOBAL, 0x00, 0x00, 0x00, 0x00, // GetGlobal 0 (console)
//         PUSH_CONST, 0x01, 0x00, 0x00, 0x00, // PushConst 1 (log)
//         GET_MEMBER, // GetMember
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         END, // End
//         CREATE_CONTEXT, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 0, 1
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x02, 0,0,0,// PushInt 2
//         LT, // Lt
//         JMP_IF_FALSE, 6, 0x00, 0x00, 0x00, // JmpIfFalse 6
//         PUSH_INT32, 0x01,0,0,0, // PushInt 1
//         RETURN, // Return
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x01,0,0,0, // PushInt 1
//         SUB, // Sub
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x02, 0,0,0,// PushInt 2
//         SUB, // Sub
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         ADD, // Add
//         RETURN, // Return
//     ]);
// }
