use crate::DotPathBuf;

pub trait Store<T>: Clone {
    fn __set(ctx: &alloc::rc::Rc<T>, base_path: DotPathBuf, value: Self);
}

pub trait Retrieve<T>: Clone {
    fn __get(ctx: &alloc::rc::Rc<T>, base_path: DotPathBuf) -> Option<Self>;
}
