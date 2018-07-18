use node::{FormalParameter, FormalParameters, Node};

use rand::{RngCore, XorShiftRng};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FreeVariableFinder {
    pub varmap: Vec<HashSet<String>>,
    pub cur_fv: HashSet<String>,
    pub mangled_function_name: Vec<HashMap<String, String>>,
    pub use_this: bool,
    pub xorshift: XorShiftRng,
}

impl FreeVariableFinder {
    pub fn new() -> FreeVariableFinder {
        let mut varmap = HashSet::new();
        varmap.insert("console".to_string());
        FreeVariableFinder {
            varmap: vec![varmap],
            cur_fv: HashSet::new(),
            mangled_function_name: vec![],
            use_this: false,
            xorshift: XorShiftRng::new_unseeded(),
        }
    }

    pub fn run_toplevel(&mut self, node: &mut Node) {
        match node {
            &mut Node::StatementList(ref mut nodes) => {
                let mut func_decl_index = vec![];

                for (i, node) in nodes.iter_mut().enumerate() {
                    match node {
                        &mut Node::FunctionDecl(ref name, _, _, _, _) => {
                            self.varmap[0].insert(name.clone());
                            func_decl_index.push(i)
                        }
                        _ => self.run(node),
                    }
                }

                for index in func_decl_index {
                    self.run(&mut nodes[index]);
                    self.use_this = false;
                }
            }
            _ => unreachable!(),
        }
    }

    fn run(&mut self, node: &mut Node) {
        match node {
            &mut Node::StatementList(ref mut nodes) => {
                for node in nodes {
                    self.run(node)
                }
            }
            &mut Node::FunctionDecl(
                ref mut name,
                ref mut use_this,
                ref mut fv,
                ref params,
                ref mut body,
            ) => {
                self.varmap.push(HashSet::new());
                self.varmap.last_mut().unwrap().insert(name.clone());

                for param in params.clone() {
                    self.varmap.last_mut().unwrap().insert(param.name);
                }

                let mut body = if let &mut Node::StatementList(ref mut body) = &mut **body {
                    body
                } else {
                    unreachable!()
                };

                self.mangled_function_name.push(HashMap::new());

                let mut func_decl_index = vec![];
                for (i, node) in body.iter_mut().enumerate() {
                    match node {
                        &mut Node::FunctionDecl(ref mut name, _, _, _, _) => {
                            let nested = self.varmap.len() + 1 > 2;
                            let mangled_name = if nested {
                                Some(format!("{}.{}", name.clone(), self.xorshift.next_u32()))
                            } else {
                                None
                            };

                            self.varmap.last_mut().unwrap().insert(name.clone());

                            if let Some(ref mangled_name) = mangled_name {
                                self.mangled_function_name
                                    .last_mut()
                                    .unwrap()
                                    .insert(name.clone(), mangled_name.clone());
                            }

                            if nested {
                                *name = mangled_name.clone().unwrap();
                            }

                            func_decl_index.push(i)
                        }
                        _ => {}
                    }
                }

                for node in body.iter_mut() {
                    match node {
                        &mut Node::FunctionDecl(_, _, _, _, _) => {}
                        _ => self.run(node),
                    }
                }

                for index in func_decl_index {
                    self.run(&mut body[index])
                }

                self.mangled_function_name.pop();

                for v in self.varmap.last().unwrap() {
                    self.cur_fv.remove(v);
                }

                *fv = self.cur_fv.clone();
                *use_this = self.use_this;

                self.varmap.pop();

                self.varmap.last_mut().unwrap().insert(name.clone());
            }
            &mut Node::Call(ref mut callee, ref mut args) => {
                self.run(callee);
                for arg in args {
                    self.run(arg)
                }
            }
            &mut Node::VarDecl(ref name, ref mut init) => {
                self.varmap.last_mut().unwrap().insert(name.clone());
                if let &mut Some(ref mut init) = init {
                    self.run(init)
                }
            }
            &mut Node::Return(ref mut val) => {
                if let &mut Some(ref mut val) = val {
                    self.run(&mut **val)
                }
            }
            &mut Node::Member(ref mut parent, _) => {
                self.run(&mut *parent);
            }
            &mut Node::This => self.use_this = true,
            &mut Node::Identifier(ref mut name) => {
                let is_fv_or_gv = !self.varmap[0].contains(name.as_str())
                    && !self.varmap.last().unwrap().contains(name.as_str());

                if let Some(mangled_function_name) = self.mangled_function_name.last() {
                    if let Some(mangled_name) = mangled_function_name.get(name.as_str()) {
                        *name = mangled_name.clone();
                    }
                }

                if is_fv_or_gv {
                    if self.varmap.len() == 1 {
                        // toplevel
                        self.varmap[0].insert(name.clone());
                    } else {
                        self.cur_fv.insert(name.clone());
                    }
                }
            }
            &mut Node::If(ref mut cond, ref mut then, ref mut else_) => {
                self.run(&mut *cond);
                self.run(&mut *then);
                self.run(&mut *else_);
            }
            &mut Node::While(ref mut cond, ref mut body) => {
                self.run(&mut *cond);
                self.run(&mut *body);
            }
            &mut Node::Assign(ref mut dst, ref mut src) => {
                match &mut **dst {
                    &mut Node::Identifier(ref name) => {
                        if !self.varmap.iter().any(|v| v.contains(name.as_str())) {
                            // If such a variable didn't appear before, this assignment
                            // serves the declaration of it as a global variable.
                            self.varmap[0].insert(name.clone());
                        } else if !self.varmap[0].contains(name.as_str())
                            && !self.varmap.last().unwrap().contains(name.as_str())
                        {
                            self.cur_fv.insert(name.clone());
                        }
                    }
                    &mut Node::Member(ref mut parent, _) => {
                        self.run(parent);
                    }
                    _ => unimplemented!(),
                }
                self.run(&mut *src);
            }
            &mut Node::UnaryOp(ref mut expr, _) => {
                self.run(&mut *expr);
            }
            &mut Node::BinaryOp(ref mut lhs, ref mut rhs, _) => {
                self.run(&mut *lhs);
                self.run(&mut *rhs);
            }
            &mut Node::TernaryOp(ref mut cond, ref mut then, ref mut else_) => {
                self.run(&mut *cond);
                self.run(&mut *then);
                self.run(&mut *else_);
            }
            _ => {}
        }
    }
}