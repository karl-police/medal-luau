use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::Read,
    rc::Rc,
};

use either::Either;
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;

use ast::{LValue, LocalRw, RValue, RcLocal, Statement};
use cfg::{block::Terminator, function::Function};

use lua51_deserializer::{
    argument::{Constant, Register, RegisterOrConstant},
    Function as BytecodeFunction, Instruction, Value,
};

use petgraph::stable_graph::NodeIndex;

pub struct LifterContext<'a> {
    bytecode: &'a BytecodeFunction<'a>,
    nodes: FxHashMap<usize, NodeIndex>,
    blocks_to_skip: FxHashSet<usize>,
    locals: FxHashMap<Register, RcLocal>,
    constants: FxHashMap<usize, ast::Literal>,
    function: Function,
}

impl<'a> LifterContext<'a> {
    fn allocate_locals(&mut self) {
        for i in 0..self.bytecode.maximum_stack_size {
            let local = self.function.local_allocator.borrow_mut().allocate();
            if i < self.bytecode.number_of_parameters {
                self.function.parameters.push(local.clone());
            }
            self.locals.insert(Register(i), local);
        }
    }

    fn create_block_map(&mut self) {
        self.nodes.insert(0, self.function.new_block());
        for (insn_index, insn) in self.bytecode.code.iter().enumerate() {
            match *insn {
                Instruction::SetList {
                    block_number: 0, ..
                } => {
                    // TODO: skip next instruction
                    todo!();
                }
                Instruction::LoadBoolean {
                    skip_next: true, ..
                } => {
                    self.nodes
                        .entry(insn_index + 2)
                        .or_insert_with(|| self.function.new_block());
                }
                Instruction::Equal { .. }
                | Instruction::LessThan { .. }
                | Instruction::LessThanOrEqual { .. }
                | Instruction::Test { .. }
                | Instruction::IterateGenericForLoop { .. } => {
                    self.nodes
                        .entry(insn_index + 1)
                        .or_insert_with(|| self.function.new_block());
                    self.nodes
                        .entry(insn_index + 2)
                        .or_insert_with(|| self.function.new_block());
                }
                Instruction::Jump(step) => {
                    let dest_block = *self
                        .nodes
                        .entry(insn_index + step as usize - 131070)
                        .or_insert_with(|| self.function.new_block());
                    self.nodes
                        .entry(insn_index + 1)
                        .or_insert_with(|| self.function.new_block());
                    if let Some(jmp_block) = self.nodes.remove(&insn_index) {
                        self.function.remove_block(jmp_block);
                        self.nodes.insert(insn_index, dest_block);
                        self.blocks_to_skip.insert(insn_index);
                    }
                }
                Instruction::IterateNumericForLoop { step, .. }
                | Instruction::PrepareNumericForLoop { step, .. } => {
                    self.nodes
                        .entry(insn_index + step as usize - 131070)
                        .or_insert_with(|| self.function.new_block());
                    self.nodes
                        .entry(insn_index + 1)
                        .or_insert_with(|| self.function.new_block());
                }
                Instruction::Return(..) => {
                    self.nodes
                        .entry(insn_index + 1)
                        .or_insert_with(|| self.function.new_block());
                }
                _ => {}
            }
        }
    }

    fn code_ranges(&self) -> Vec<(usize, usize)> {
        let mut nodes = self.nodes.keys().cloned().collect::<Vec<_>>();
        nodes.sort_unstable();
        let ends = nodes
            .iter()
            .skip(1)
            .map(|&s| s - 1)
            .chain(std::iter::once(self.bytecode.code.len() - 1));
        nodes
            .iter()
            .cloned()
            .zip(ends)
            .filter(|(s, _)| !self.blocks_to_skip.contains(s))
            .collect()
    }

    fn constant(&mut self, constant: Constant) -> ast::Literal {
        let converted_constant = match self.bytecode.constants.get(constant.0 as usize).unwrap() {
            Value::Nil => ast::Literal::Nil,
            Value::Boolean(v) => ast::Literal::Boolean(*v),
            Value::Number(v) => ast::Literal::Number(*v),
            Value::String(v) => ast::Literal::String(v.to_string()),
        };
        self.constants
            .entry(constant.0 as usize)
            .or_insert(converted_constant)
            .clone()
    }

    fn register_or_constant(&mut self, value: RegisterOrConstant) -> ast::RValue {
        match value.0 {
            Either::Left(register) => self.locals[&register].clone().into(),
            Either::Right(constant) => self.constant(constant).into(),
        }
    }

    fn lift_instruction(&mut self, start: usize, end: usize, statements: &mut Vec<Statement>) {
        let mut table_to_index: HashMap<Register, usize> = HashMap::new();
        let mut table_to_definition: HashMap<Register, usize> = HashMap::new();

        for (index, instruction) in self.bytecode.code[start..=end].iter().enumerate() {
            match instruction {
                Instruction::Move {
                    destination,
                    source,
                } => {
                    statements.push(
                        ast::Assign {
                            left: vec![(self.locals[&destination].clone().into(), None)],
                            right: vec![self.locals[&source].clone().into()],
                        }
                        .into(),
                    );
                }
                &Instruction::LoadBoolean {
                    destination, value, ..
                } => {
                    statements.push(
                        ast::Assign {
                            left: vec![(self.locals[&destination].clone().into(), None)],
                            right: vec![ast::Literal::Boolean(value).into()],
                        }
                        .into(),
                    );
                }
                &Instruction::LoadConstant {
                    destination,
                    source,
                } => {
                    statements.push(
                        ast::Assign {
                            left: vec![(self.locals[&destination].clone().into(), None)],
                            right: vec![self.constant(source).into()],
                        }
                        .into(),
                    );
                }
                Instruction::LoadNil(registers) => {
                    for register in registers {
                        statements.push(
                            ast::Assign::new(
                                vec![self.locals[&register].clone().into()],
                                vec![ast::Literal::Nil.into()],
                            )
                            .into(),
                        );
                    }
                }
                &Instruction::GetGlobal {
                    destination,
                    global,
                } => {
                    let global_str = self.constant(global).as_string().unwrap().clone();
                    statements.push(
                        ast::Assign::new(
                            vec![self.locals[&destination].clone().into()],
                            vec![ast::Global::new(global_str).into()],
                        )
                        .into(),
                    );
                }
                &Instruction::SetGlobal { destination, value } => {
                    let global_str = self.constant(destination).as_string().unwrap().clone();
                    statements.push(
                        ast::Assign::new(
                            vec![ast::Global::new(global_str).into()],
                            vec![self.locals[&value].clone().into()],
                        )
                        .into(),
                    );
                }
                &Instruction::GetTable {
                    destination,
                    table,
                    key,
                } => {
                    statements.push(
                        ast::Assign::new(
                            vec![self.locals[&destination].clone().into()],
                            vec![ast::Index::new(
                                self.locals[&table].clone().into(),
                                self.register_or_constant(key),
                            )
                            .into()],
                        )
                        .into(),
                    );
                }
                &Instruction::Test {
                    value,
                    comparison_value,
                } => {
                    let value = self.locals[&value].clone().into();
                    let condition = if comparison_value {
                        value
                    } else {
                        ast::Unary::new(value, ast::UnaryOperation::Not).into()
                    };
                    statements.push(ast::If::new(condition, None, None).into())
                }
                Instruction::Not {
                    destination,
                    operand,
                } => {
                    statements.push(
                        ast::Assign::new(
                            vec![self.locals[destination].clone().into()],
                            vec![ast::Unary::new(
                                self.locals[operand].clone().into(),
                                ast::UnaryOperation::Not,
                            )
                            .into()],
                        )
                        .into(),
                    );
                }
                Instruction::Return(values, _variadic) => {
                    statements.push(
                        ast::Return::new(
                            values
                                .into_iter()
                                .map(|v| self.locals[v].clone().into())
                                .collect(),
                        )
                        .into(),
                    );
                }
                Instruction::Jump(..) => {}
                &Instruction::Add {
                    destination,
                    lhs,
                    rhs,
                }
                | &Instruction::Sub {
                    destination,
                    lhs,
                    rhs,
                }
                | &Instruction::Mul {
                    destination,
                    lhs,
                    rhs,
                }
                | &Instruction::Div {
                    destination,
                    lhs,
                    rhs,
                }
                | &Instruction::Mod {
                    destination,
                    lhs,
                    rhs,
                }
                | &Instruction::Pow {
                    destination,
                    lhs,
                    rhs,
                } => {
                    statements.push(
                        ast::Assign::new(
                            vec![self.locals[&destination].clone().into()],
                            vec![ast::Binary::new(
                                self.register_or_constant(lhs),
                                self.register_or_constant(rhs),
                                match instruction {
                                    Instruction::Add { .. } => ast::BinaryOperation::Add,
                                    Instruction::Sub { .. } => ast::BinaryOperation::Sub,
                                    Instruction::Mul { .. } => ast::BinaryOperation::Mul,
                                    Instruction::Div { .. } => ast::BinaryOperation::Div,
                                    Instruction::Mod { .. } => ast::BinaryOperation::Mod,
                                    Instruction::Pow { .. } => ast::BinaryOperation::Pow,
                                    _ => unreachable!(),
                                },
                            )
                            .into()],
                        )
                        .into(),
                    );
                }
                Instruction::TestSet {
                    destination,
                    value,
                    comparison_value,
                } => {
                    let value: ast::RValue = self.locals[&value].clone().into();
                    let assign = ast::Assign {
                        left: vec![(self.locals[&destination].clone().into(), None)],
                        right: vec![value.clone()],
                    };
                    let new_block = self.function.new_block();

                    statements.push(
                        ast::If {
                            condition: Box::new(if *comparison_value {
                                ast::Unary {
                                    value: Box::new(value.clone()),
                                    operation: ast::UnaryOperation::Not,
                                }
                                .into()
                            } else {
                                value.clone()
                            }),
                            then_block: None,
                            else_block: None,
                        }
                        .into(),
                    );

                    let condition_block = self.nodes[&start];
                    let next_block = self.nodes[&(end + 1)];
                    let step = match &self.bytecode.code[end] {
                        Instruction::Jump(step) => *step as usize,
                        _ => unreachable!(),
                    };

                    self.function
                        .block_mut(new_block)
                        .unwrap()
                        .ast
                        .push(assign.into());

                    self.function.set_block_terminator(
                        condition_block,
                        Some(Terminator::conditional(next_block, new_block)),
                    );

                    self.function.set_block_terminator(
                        new_block,
                        Some(Terminator::jump(
                            self.nodes[&(end + step as usize - 131070)],
                        )),
                    );
                }
                Instruction::Call {
                    function,
                    arguments,
                    return_values,
                } => {
                    let call = ast::Call {
                        value: Box::new(self.locals[function].clone().into()),
                        arguments: if *arguments <= 1 {
                            Vec::new()
                        } else {
                            (1..*arguments)
                                .map(|argument| {
                                    self.locals[&Register(function.0 + argument)].clone().into()
                                })
                                .collect_vec()
                        },
                    };

                    statements.push(if *return_values > 1 {
                        ast::Assign {
                            left: (0..return_values - 1)
                                .map(|return_value| {
                                    (
                                        self.locals[&Register(function.0 + return_value)]
                                            .clone()
                                            .into(),
                                        None,
                                    )
                                })
                                .collect_vec(),
                            right: vec![call.into()],
                        }
                        .into()
                    } else {
                        call.into()
                    })
                }
                Instruction::GetUpvalue {
                    destination,
                    upvalue,
                } => {
                    statements.push(
                        ast::Assign {
                            left: vec![(self.locals[destination].clone().into(), None)],
                            right: vec![RcLocal::new(Rc::new(ast::Local(Some(
                                self.bytecode.upvalues[upvalue.0 as usize].to_string(),
                            ))))
                            .into()],
                        }
                        .into(),
                    );
                }
                Instruction::Closure {
                    destination,
                    function,
                } => {
                    let closure = Self::lift(&self.bytecode.closures[function.0 as usize]);
                    let parameters = closure.parameters.clone();
                    let body = restructure::lift(closure);

                    statements.push(
                        ast::Assign {
                            left: vec![(self.locals[destination].clone().into(), None)],
                            right: vec![ast::Closure {
                                parameters,
                                body,
                                upvalues: Vec::new(),
                            }
                            .into()],
                        }
                        .into(),
                    );
                }
                &Instruction::NewTable { destination, .. } => {
                    table_to_index.insert(destination, statements.len());
                }
                Instruction::SetList {
                    table,
                    number_of_elements,
                    block_number,
                } => {
                    let mut elements = Vec::new();
                    let mut original_to_new: HashMap<_, RcLocal> = HashMap::new();
                    let mut i = 0;

                    statements.retain_mut(|statement| {
                        if i < table_to_index[table] {
                            i += 1;

                            return true;
                        }

                        if let Statement::Assign(assign) = statement {
                            if let Some((LValue::Index(ast::Index { box right, .. }), _)) =
                                assign.left.first()
                            {
                                if let RValue::Literal(ast::Literal::String(field)) = &right {
                                    elements.push((
                                        Some(field.clone()),
                                        assign.right.first().unwrap().clone(),
                                    ));
                                }

                                return false;
                            }

                            if !matches!(assign.right.first(), Some(RValue::Local(_))) {
                                let count = assign.values_read().count();

                                if elements.len() >= count {
                                    elements.drain(elements.len() - count..).collect_vec();
                                }
                            }
                        }

                        for v in statement.values_read_mut() {
                            if let Some(new_local) = original_to_new.get(v) {
                                *v = new_local.clone();
                            }
                        }

                        for v in statement.values_written_mut() {
                            let is_not_self = v != &self.locals[table];

                            if *block_number > 1 {
                                let mut new_local =
                                    self.function.local_allocator.borrow_mut().allocate();
                                std::mem::swap(v, &mut new_local);
                                original_to_new.insert(new_local, v.clone());
                            }

                            if is_not_self {
                                elements.push((None, v.clone().into()));
                            }
                        }

                        i += 1;

                        true
                    });

                    if *number_of_elements == 0 {
                        let variadic_expression: RValue = match &self.bytecode.code[index - 1] {
                            Instruction::VarArg(_) => ast::VarArg.into(), // TODO: lift vararg
                            _ => match statements.pop().unwrap() {
                                Statement::Call(call) => call.into(),
                                _ => unreachable!(),
                            },
                        };
                        let values_read = variadic_expression.values_read().count();

                        elements.drain(elements.len() - values_read..);
                        elements.push((None, variadic_expression));
                    }

                    if *block_number > 1 {
                        let mut table_assignment =
                            statements.remove(table_to_definition.remove(table).unwrap());
                        let table = match &mut table_assignment {
                            Statement::Assign(assign) => match assign.right.first_mut().unwrap() {
                                RValue::Table(table) => table,
                                _ => unreachable!(),
                            },
                            _ => unreachable!(),
                        };

                        table.0.extend(elements.into_iter());
                        statements.push(table_assignment);
                    } else {
                        table_to_definition.insert(*table, statements.len());
                        statements.push(
                            ast::Assign {
                                left: vec![(self.locals[table].clone().into(), None)],
                                right: vec![ast::Table(elements).into()],
                            }
                            .into(),
                        );
                    }
                }
                &Instruction::SetTable { table, key, value } => {
                    let key = self.register_or_constant(key);
                    let value = self.register_or_constant(value);

                    statements.push(
                        ast::Assign {
                            left: vec![(
                                ast::Index {
                                    left: Box::new(self.locals[&table].clone().into()),
                                    right: Box::new(key),
                                }
                                .into(),
                                None,
                            )],
                            right: vec![value],
                        }
                        .into(),
                    );
                }
                _ => statements.push(ast::Comment::new(format!("{:?}", instruction)).into()),
            }

            if matches!(instruction, Instruction::Return { .. }) {
                break;
            }
        }
    }

    fn lift_blocks(&mut self) {
        let ranges = self.code_ranges();
        for (start, end) in ranges {
            let mut block = ast::Block::default();

            self.lift_instruction(start, end, &mut block);
            self.function
                .block_mut(self.nodes[&start])
                .unwrap()
                .ast
                .extend(block.0);

            match self.bytecode.code[end] {
                Instruction::Equal { .. }
                | Instruction::LessThan { .. }
                | Instruction::LessThanOrEqual { .. }
                | Instruction::Test { .. }
                | Instruction::IterateGenericForLoop { .. } => {
                    self.function.set_block_terminator(
                        self.nodes[&start],
                        Some(Terminator::conditional(
                            self.nodes[&(end + 1)],
                            self.nodes[&(end + 2)],
                        )),
                    );
                }
                Instruction::Jump(step)
                | Instruction::IterateNumericForLoop { step, .. }
                | Instruction::PrepareNumericForLoop { step, .. } => {
                    let block = self.nodes[&start];

                    if self.function.block(block).unwrap().terminator.is_none() {
                        self.function.set_block_terminator(
                            block,
                            Some(Terminator::jump(
                                self.nodes[&(end + step as usize - 131070)],
                            )),
                        );
                    }
                }
                Instruction::Return { .. } => {}
                Instruction::LoadBoolean { skip_next, .. } => {
                    let successor = self.nodes[&(end + 1 + skip_next as usize)];

                    self.function.set_block_terminator(
                        self.nodes[&start],
                        Some(Terminator::jump(successor)),
                    );
                }
                _ => {
                    if end + 1 != self.bytecode.code.len() {
                        self.function.set_block_terminator(
                            self.nodes[&start],
                            Some(Terminator::jump(self.nodes[&(end + 1)])),
                        );
                    }
                }
            }
        }
    }

    pub fn lift(bytecode: &'a BytecodeFunction) -> Function {
        let mut context = Self {
            bytecode,
            nodes: FxHashMap::default(),
            locals: FxHashMap::default(),
            constants: FxHashMap::default(),
            function: Function::default(),
            blocks_to_skip: FxHashSet::default(),
        };

        context.create_block_map();
        context.allocate_locals();
        context.lift_blocks();
        for node in context
            .function
            .graph()
            .node_indices()
            .filter(|&i| i != context.nodes[&0])
            .collect::<Vec<_>>()
        {
            if context.function.predecessor_blocks(node).next().is_none() {
                context.function.remove_block(node);
            }
        }
        context.function.set_entry(context.nodes[&0]);

        context.function
    }
}
