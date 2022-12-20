use std::marker::PhantomData;

use super::*;

pub struct DirectCall;

#[derive(Debug)]
pub enum Op {
    Litr(i64),
    Arg(usize),
    Get(usize),
    Add,
    PushLocal,
    PopLocal,
    SetLocal(usize),
    Pop,
    JmpZN(usize),
    Jmp(usize),
    Ret,
}

type InsnFunc = fn(&[i64], &mut usize, &mut Vec<i64>, &mut Vec<i64>, u64) -> bool;

fn litr(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    stack.push(data as i64);
    false
}
fn arg(
    args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    unsafe {
        stack.push(*args.get_unchecked(data as usize));
    }
    false
}
fn get(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    unsafe { stack.push(*locals.get_unchecked(locals.len() - data as usize - 1)) }
    false
}
fn add(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    _data: u64,
) -> bool {
    unsafe {
        let x = stack.pop().unwrap_unchecked();
        let y = stack.pop().unwrap_unchecked();
        stack.push(x + y);
    }
    false
}
fn push_local(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    locals: &mut Vec<i64>,
    _data: u64,
) -> bool {
    unsafe {
        locals.push(stack.pop().unwrap_unchecked());
    }
    false
}
fn pop_local(
    _args: &[i64],
    _pc: &mut usize,
    _stack: &mut Vec<i64>,
    locals: &mut Vec<i64>,
    _data: u64,
) -> bool {
    unsafe {
        locals.pop().unwrap_unchecked();
    }
    false
}
fn set_local(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    unsafe {
        let rhs = stack.pop().unwrap_unchecked();
        let local_offs = locals.len() - data as usize - 1;
        unsafe {
            *locals.get_unchecked_mut(local_offs) = rhs;
        }
    }
    false
}
fn pop(
    _args: &[i64],
    _pc: &mut usize,
    stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    _data: u64,
) -> bool {
    unsafe {
        stack.pop().unwrap_unchecked();
    }
    false
}
fn jmp_zn(
    _args: &[i64],
    pc: &mut usize,
    stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    unsafe {
        if stack.pop().unwrap_unchecked() <= 0 {
            *pc = data as usize;
        }
    }
    false
}
fn jmp(
    _args: &[i64],
    pc: &mut usize,
    _stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    data: u64,
) -> bool {
    unsafe {
        *pc = data as usize;
    }
    false
}
fn ret(
    _args: &[i64],
    _pc: &mut usize,
    _stack: &mut Vec<i64>,
    _locals: &mut Vec<i64>,
    _data: u64,
) -> bool {
    true
}

pub struct Program<'a> {
    insns: Vec<(InsnFunc, u64)>,
    phantom: PhantomData<&'a ()>,
}

impl Vm for DirectCall {
    type Program<'a> = Program<'a>;

    fn compile(expr: &Expr) -> Self::Program<'_> {
        fn returns(expr: &Expr) -> bool {
            match expr {
                Expr::Litr(_) | Expr::Arg(_) | Expr::Get(_) | Expr::Add(_, _) => true,
                Expr::Let(_, expr) => returns(expr),
                Expr::Set(_, _) | Expr::While(_, _) => false,
                Expr::Then(_, b) => returns(b),
            }
        }

        fn compile_inner(prog: &mut Program, expr: &Expr) {
            match expr {
                Expr::Litr(x) => {
                    prog.insns.push((litr, *x as u64));
                }
                Expr::Arg(idx) => {
                    prog.insns.push((arg, *idx as u64));
                }
                Expr::Get(local) => {
                    prog.insns.push((get, *local as u64));
                }
                Expr::Add(x, y) => {
                    compile_inner(prog, x);
                    compile_inner(prog, y);
                    prog.insns.push((add, 0));
                }
                Expr::Let(rhs, then) => {
                    compile_inner(prog, rhs);
                    prog.insns.push((push_local, 0));
                    compile_inner(prog, then);
                    prog.insns.push((pop_local, 0));
                }
                Expr::Set(local, rhs) => {
                    compile_inner(prog, rhs);
                    prog.insns.push((set_local, *local as u64));
                }
                Expr::While(pred, body) => {
                    let start = prog.insns.len();
                    compile_inner(prog, pred);
                    let branch_fixup = prog.insns.len();
                    prog.insns.push((jmp_zn, 0)); // Will be fixed up
                    compile_inner(prog, body);
                    if returns(body) {
                        prog.insns.push((pop, 0));
                    }
                    prog.insns.push((jmp, start as u64));
                    prog.insns[branch_fixup].1 = prog.insns.len() as u64;
                }
                Expr::Then(a, b) => {
                    compile_inner(prog, a);
                    if returns(a) {
                        prog.insns.push((pop, 0));
                    }
                    compile_inner(prog, b);
                }
            }
        }

        let mut prog = Program {
            insns: Vec::new(),
            phantom: PhantomData,
        };

        compile_inner(&mut prog, expr);

        prog.insns.push((ret, 0));

        prog
    }

    unsafe fn execute(prog: &Self::Program<'_>, args: &[i64]) -> i64 {
        let mut ip = 0;
        let mut stack = Vec::new();
        let mut locals = Vec::new();

        let insns = prog.insns.as_slice();
        loop {
            let (func, d) = *insns.get_unchecked(ip);
            ip += 1;
            if func(args, &mut ip, &mut stack, &mut locals, d) {
                break stack.pop().unwrap_unchecked();
            }
        }
    }
}
