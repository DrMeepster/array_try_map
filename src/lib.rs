//! Adds [`try_map`](ArrayExt::try_map) and [`map2`](ArrayExt::try_map) methods to arrays.
//!
//! This crate requires nightly.

#![no_std]
#![feature(
    min_const_generics,
    maybe_uninit_uninit_array,
    maybe_uninit_extra,
    maybe_uninit_slice,
    array_value_iter,
    never_type,
    unwrap_infallible
)]
#![deny(missing_docs)]

/// Extension of `[T; N]` to add methods
pub trait ArrayExt<T, const N: usize> {
    /// Fallible version of `map`.
    /// The provided function will be run on every element until the array ends or an error is returned.
    ///
    /// # Errors
    /// 
    /// If `f` returns an [`Err`], that error will be returned by this function.
    /// The already initialized elements will be dropped when an error occurs.
    /// The new array will be returned if no error occurs.
    /// 
    /// # Panics
    ///
    /// This function panics if `f` panics.
    /// The already initialized elements will be dropped when a panic occurs.
    ///
    /// # Examples
    ///
    /// ```
    ///  # use array_try_map::ArrayExt;
    /// let x: [u32; 3] = [1, 2, 3];
    /// let y = x.try_map(|v| v.checked_add(1).ok_or("overflow"));
    /// assert_eq!(y, Ok([2, 3, 4]));
    ///
    /// let x = [1, 2, 3, u32::MAX];
    /// let y = x.try_map(|v| v.checked_add(1).ok_or("overflow"));
    /// assert_eq!(y, Err("overflow"));
    /// ```
    fn try_map<F, U, E>(self, f: F) -> Result<[U; N], E>
    where
        F: FnMut(T) -> Result<U, E>;

    /// Example of how `map` could be reimplemented in terms of [`try_map`](ArrayExt::try_map).
    ///
    /// # Panics
    ///
    /// This function panics if `f` panics.
    /// The already initialized elements will be dropped when a panic occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use array_try_map::ArrayExt;
    /// let x = [1, 2, 3];
    /// let y = x.map2(|v| v + 1);
    /// assert_eq!(y, [2, 3, 4]);
    ///
    /// let x = [1, 2, 3];
    /// let mut temp = 0;
    /// let y = x.map2(|v| { temp += 1; v * temp });
    /// assert_eq!(y, [1, 4, 9]);
    ///
    /// let x = ["Ferris", "Bueller's", "Day", "Off"];
    /// let y = x.map2(|v| v.len());
    /// assert_eq!(y, [6, 9, 3, 3]);
    /// ```
    fn map2<F, U>(self, f: F) -> [U; N]
    where
        F: FnMut(T) -> U;
}

impl<T, const N: usize> ArrayExt<T, N> for [T; N] {
    // code here is modified code from core
    fn try_map<F, U, E>(self, mut f: F) -> Result<[U; N], E>
    where
        F: FnMut(T) -> Result<U, E>,
    {
        use core::mem::MaybeUninit;
        struct Guard<T, const N: usize> {
            dst: *mut T,
            initialized: usize,
        }

        impl<T, const N: usize> Drop for Guard<T, N> {
            fn drop(&mut self) {
                debug_assert!(self.initialized <= N);

                let initialized_part =
                    core::ptr::slice_from_raw_parts_mut(self.dst, self.initialized);
                // SAFETY: this raw slice will contain only initialized objects
                // that's why, it is allowed to drop it.
                unsafe {
                    core::ptr::drop_in_place(initialized_part);
                }
            }
        }
        let mut dst = MaybeUninit::uninit_array::<N>();
        let mut guard: Guard<U, N> = Guard {
            dst: MaybeUninit::slice_as_mut_ptr(&mut dst),
            initialized: 0,
        };
        for (src, dst) in core::array::IntoIter::new(self).zip(&mut dst) {
            //CHANGED FROM CORE: match on `f(src)` instead of directly inputting it into `dst.write`
            match f(src) {
                Ok(elem) => {
                    dst.write(elem);
                    guard.initialized += 1;
                }
                Err(err) => return Err(err),
            }
        }
        // FIXME: Convert to crate::mem::transmute once it works with generics.
        // unsafe { crate::mem::transmute::<[MaybeUninit<U>; N], [U; N]>(dst) }
        core::mem::forget(guard);
        // SAFETY: At this point we've properly initialized the whole array
        // and we just need to cast it to the correct type.
        Ok(unsafe { core::mem::transmute_copy::<_, [U; N]>(&dst) }) //CHANGED FROM CORE: Ok-wrapped
    }

    fn map2<F, U>(self, mut f: F) -> [U; N]
    where
        F: FnMut(T) -> U,
    {
        self.try_map::<_, _, !>(|src| Ok(f(src))).into_ok()
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::ArrayExt;

    use std::{
        mem, panic,
        rc::Rc,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[test]
    /// Tests that if the function returns an error, the initalized contents of the array will be dropped.
    fn drop_on_err() {
        let x = [0, 0, 0, 0, 255];
        let rc = Rc::new(());

        let _ = x.try_map(|i| if i == 0 { Ok(rc.clone()) } else { Err(()) });

        assert_eq!(Rc::strong_count(&rc), 1);
    }

    #[test]
    /// Tests that if the function panics, the initalized contents of the array will be dropped. 
    fn drop_on_panic() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        struct ObjectCounter;

        impl ObjectCounter {
            fn new() -> Self {
                COUNTER.fetch_add(1, Ordering::AcqRel);
                Self
            }
        }

        impl Drop for ObjectCounter {
            fn drop(&mut self) {
                COUNTER.fetch_sub(1, Ordering::AcqRel);
            }
        }

        let f = |i| {
            if i == 0 {
                ObjectCounter::new()
            } else {
                panic!("expected panic")
            }
        };

        let x = [0, 0, 0, 0];

        let res = panic::catch_unwind(move || {
            x.map2(f)
        });

        assert_eq!(COUNTER.load(Ordering::Acquire), 4);

        mem::drop(res);

        let x = [0, 0, 0, 0, 255];

        let _res = panic::catch_unwind(move || {
            x.map2(f)
        });

        assert_eq!(COUNTER.load(Ordering::Acquire), 0);
    }

    /// Tests that the function does not run after an error occurs.
    #[test]
    fn short_circuit(){
        let mut counter = 0;

        let x = [0,0,0,0];

        let _ = x.try_map(|i|{
            if i == 0 {
                counter += 1;
                Ok(())
            }else{
                Err(())
            }
        });

        assert_eq!(counter, 4);

        counter = 0;

        let y = [0,0,255,0,0];

        let _ = y.try_map(|i|{
            if i == 0 {
                counter += 1;
                Ok(())
            }else{
                Err(())
            }
        });

        assert_eq!(counter, 2);
    }
}
