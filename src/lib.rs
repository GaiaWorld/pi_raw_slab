//! 自动扩展的内存板VecSlab。仅在wasm环境下使用的。
//! 自动扩展的内存板(VBSlab)。在多线程环境下使用的。但各种访问方法，要求外部保证，多线程调用时不要同时访问到同一个元素，否则会引发数据竞争。
//! 由一个主内存板(可扩展)和多个固定大小的辅助内存板构成。
//! 当主内存板上的Vec长度不够时，不会立刻扩容Vec，而是线程安全的在辅助内存板分配新Vec。
//! 在独占整理时，会合并所有内存板数据到主内存板。
//! 

#![feature(unsafe_cell_access)]
#![feature(vec_into_raw_parts)]
#![feature(test)]
extern crate test;

use std::cell::UnsafeCell;
use std::mem::{replace, transmute};
use std::ptr::NonNull;

use pi_buckets::{Buckets, Location, BUCKETS};
use pi_vec_remain::VecRemain;

#[cfg(feature = "rc")]
pub type RawSlab = VecSlab;

#[cfg(not(feature = "rc"))]
pub type RawSlab = VBSlab;

pub struct VBSlab {
    raw_size: usize, //每个元素的大小
    ptr: *mut u8,
    capacity: usize, // ptr所在vec的容量
    buckets: *mut Buckets<u8>,
}
unsafe impl Send for VBSlab {}
unsafe impl Sync for VBSlab {}
impl Default for VBSlab {
    fn default() -> Self {
        VBSlab::with_capacity(0, 0)
    }
}

impl VBSlab {

    pub fn with_capacity(raw_size: usize, capacity: usize) -> VBSlab {
        if raw_size == 0 {
            return VBSlab {
                raw_size,
                ptr: NonNull::<u8>::dangling().as_ptr(),
                capacity: usize::MAX,
                buckets: NonNull::<Buckets<u8>>::dangling().as_ptr(),
            };
        }
        let ptr = if capacity == 0 {
            NonNull::<u8>::dangling().as_ptr()
        } else {
            let vec = Vec::with_capacity(capacity * raw_size);
            vec.into_raw_parts().0
        };
        let buckets = Box::into_raw(Box::new(Default::default()));
        VBSlab {
            raw_size,
            ptr,
            capacity,
            buckets,
        }
    }
    /// 获得容量大小
    #[inline(always)]
    pub fn capacity(&self, len: usize) -> usize {
        if len > self.capacity {
            Location::bucket_capacity(Location::bucket(len - self.capacity)) + self.capacity
        } else {
            self.capacity
        }
    }

    #[inline(always)]
    pub fn vec_capacity(&self) -> usize {
        self.capacity
    }
    #[inline(always)]
    fn buckets(&self) -> &Buckets<u8> {
        unsafe { &*self.buckets }
    }
    #[inline]
    pub fn get<T>(&self, index: usize) -> Option<&mut T> {
        if index < self.vec_capacity() {
            return Some(unsafe { transmute(&mut *self.ptr.add(index * self.raw_size)) });
        }
        let mut loc = Location::of(index - self.capacity);
        loc.entry *= self.raw_size;
        unsafe { transmute(self.buckets().load(&loc)) }
    }

    #[inline]
    pub fn get_unchecked<T>(&self, index: usize) -> &mut T {
        if index < self.vec_capacity() {
            return unsafe { transmute(&mut *self.ptr.add(index * self.raw_size)) };
        }
        let mut loc = Location::of(index - self.capacity);
        loc.entry *= self.raw_size;
        unsafe { transmute(self.buckets().load_unchecked(&loc)) }
    }

    #[inline]
    pub fn load_alloc<T>(&self, index: usize) -> &mut T {
        if index < self.vec_capacity() {
            return unsafe { transmute(&mut *self.ptr.add(index * self.raw_size)) };
        }
        let mut loc = Location::of(index - self.capacity);
        loc.entry *= self.raw_size;
        loc.len *= self.raw_size;
        unsafe { transmute(self.buckets().load_alloc(&loc)) }
    }
    /// 整理内存
    pub fn settle(&mut self, len: usize) {
        // println!("settle: {:?}", (self.capacity, self.ptr, len));
        if self.raw_size == 0 {
            return;
        }
        if len <= self.capacity {
            // 数据都在vec上
            return;
        }
        let vec_len = self.capacity * self.raw_size;
        let mut vec = unsafe { Vec::from_raw_parts(self.ptr, vec_len, vec_len) };
        // 取出所有的bucket
        let mut arr = self.take_buckets();
        // 获得最后一个bucket的索引
        let bucket_end = Location::bucket(len - self.capacity);
        if vec.capacity() == 0 && bucket_end == 0 {
            // 如果vec为空，并且bucket_end为0，则表示只有第一个槽有数据，则将vec交换成第一个槽的数据
            vec = replace(&mut arr[0], Vec::new());
            self.capacity = vec.capacity() / self.raw_size;
            self.ptr = vec.into_raw_parts().0;
            return;
        }
        let mut start = vec.capacity();
        // 获得扩容后的总容量
        let cap = (Location::bucket_capacity(bucket_end) + self.capacity) * self.raw_size;
        // 如果vec容量小于cap，则将vec容量扩展到cap
        if vec.capacity() < cap {
            vec.reserve(cap - vec.capacity());
        }
        let end = len * self.raw_size;
        // println!("1111111111111, bucket:{:?}, {:?}", (bucket_start, bucket_end), (start, end));
        // 将arr的数据拷贝到vec上
        for (i, v) in arr[0..bucket_end + 1].iter_mut().enumerate() {
            let mut vlen = v.len();
            if vlen > 0 {
                v.remain_to(0..end - start, &mut vec);
            } else {
                vlen = Location::bucket_len(i) * self.raw_size;
            }
            start += vlen;
        }
        self.capacity = vec.capacity() / self.raw_size;
        self.ptr = vec.into_raw_parts().0;
        // println!("222222222, capacity:{:?}, ptr:{:?}", (self.capacity), (self.ptr));
    }

    fn take_buckets(&mut self) -> [Vec<u8>; BUCKETS] {
        // 取出所有的bucket
        let buckets = self.buckets().take();
        buckets.map(|vec| {
            let len = vec.len() * self.raw_size;
            let ptr = vec.into_raw_parts().0;
            unsafe { Vec::from_raw_parts(ptr, len, len) }
        })
    }
}

pub struct VecSlab {
    raw_size: usize, // 每个元素的大小
    ptr: UnsafeCell<*mut u8>,
    capacity: UnsafeCell<usize>,
}

impl VecSlab {
    pub fn with_capacity(raw_size: usize, capacity: usize) -> VecSlab {
        if raw_size == 0 {
            return Self {
                raw_size,
                ptr: NonNull::<u8>::dangling().as_ptr().into(),
                capacity: usize::MAX.into(),
            };
        }
        let ptr = if capacity == 0 {
            NonNull::<u8>::dangling().as_ptr().into()
        } else {
            let vec: Vec<u8> = Vec::with_capacity(capacity * raw_size);
            vec.into_raw_parts().0.into()
        };
        return VecSlab {
            raw_size,
            ptr,
            capacity: capacity.into(),
        };
    }
    /// 获得容量大小
    #[inline(always)]
    pub fn capacity(&self, _len: usize) -> usize {
        self.vec_capacity()
    }
    #[inline(always)]
    pub fn vec_capacity(&self) -> usize {
        *unsafe { self.capacity.as_ref_unchecked() }
    }

    #[inline]
    pub fn get<T>(&self, index: usize) -> Option<&mut T> {
        if index < self.vec_capacity() {
            return Some(unsafe { transmute(&mut *(*self.ptr.get()).add(index * self.raw_size)) });
        }
        None
    }
    #[inline]
    pub fn get_unchecked<T>(&self, index: usize) -> &mut T {
        debug_assert!(index < self.vec_capacity());
        unsafe { transmute(&mut *(*self.ptr.get()).add(index * self.raw_size)) }
    }
    #[inline]
    pub fn load_alloc<T>(&self, index: usize) -> &mut T {
        let capacity = self.vec_capacity();
        if index >= capacity {
            let len = capacity * self.raw_size;
            let vec = unsafe { Vec::from_raw_parts(*self.ptr.get(), len, len) };

            self.reserve(vec, capacity, index - capacity + 1);
        }
        return unsafe { transmute(&mut *(*self.ptr.get()).add(index * self.raw_size)) };
    }

    /// 整理内存
    pub fn settle(&mut self, _len: usize) {}

    /// 为给定的向量vec预留空间，len为预留的长度，additional为额外的预留空间
    fn reserve(&self, mut vec: Vec<u8>, len: usize, mut additional: usize) {
        // 计算需要预留的空间大小
        additional = (len + additional).saturating_sub(self.vec_capacity());
        // 如果需要预留的空间大于0
        if additional > 0 {
            // 为向量预留空间
            vec.reserve(additional * self.raw_size);
            // 更新容量
            unsafe { self.capacity.replace(vec.capacity() / self.raw_size) };
        }
        // 将向量的指针替换为预留空间的指针
        unsafe { self.ptr.replace(vec.into_raw_parts().0) };
    }
}

impl Default for VecSlab {
    fn default() -> Self {
        VecSlab::with_capacity(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use pcg_rand::Pcg64;
    use rand::{Rng, SeedableRng};
    use crate::*;

    #[test]
    fn test3() {
        let mut arr = RawSlab::with_capacity(size_of::<usize>(), 0);
        let mut i = 0;
        // let mut rng = rand::thread_rng();
        let mut rng = Pcg64::seed_from_u64(2);
        for _ in 0..1000 {
            let x = rng.gen_range(0..1000);
            // println!("test3 start: {:?}", x);
            for _ in 0..x {
                let r: &mut usize =
                    unsafe { transmute(arr.load_alloc::<usize>(i)) };
                *r = i;
                i += 1;
            }
            check3(&arr, i);
            arr.settle(i);
            check3(&arr, i);
            if rng.gen_range(0..200) == 0 {
                println!("reset ----------");
                arr = RawSlab::with_capacity(size_of::<usize>(), 0);
                i = 0;
            }
        }
        // println!("test3 arr.vec_capacity(): {}", arr.vec_capacity());
    }
    fn check3(arr: &RawSlab, len: usize) {
        for i in 0..len {
            let r: &mut usize = unsafe { transmute(arr.get::<usize>(i)) };
            assert_eq!(*r, i);
        }
    }
}
