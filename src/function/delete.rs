use std::ptr::NonNull;

use crossbeam_queue::SegQueue;

use crate::function::Configuration;
use crate::function::memo::Memo;

/// Stores the list of memos that have been deleted so they can be freed
/// once the next revision starts. See the comment on the field
/// `deleted_entries` of [`FunctionIngredient`][] for more details.
pub(super) struct DeletedEntries<C: Configuration> {
    memos: boxcar::Vec<SharedBox<Memo<'static, C>>>,
    values_to_drop: SegQueue<SharedPtr<Memo<'static, C>>>,
}

#[allow(clippy::undocumented_unsafe_blocks)] // TODO(#697) document safety
unsafe impl<T: Send> Send for SharedBox<T> {}
#[allow(clippy::undocumented_unsafe_blocks)] // TODO(#697) document safety
unsafe impl<T: Sync> Sync for SharedBox<T> {}

#[allow(clippy::undocumented_unsafe_blocks)] // TODO(#697) document safety
unsafe impl<T: Send> Send for SharedPtr<T> {}
#[allow(clippy::undocumented_unsafe_blocks)] // TODO(#697) document safety
unsafe impl<T: Sync> Sync for SharedPtr<T> {}

impl<C: Configuration> Default for DeletedEntries<C> {
    fn default() -> Self {
        Self {
            memos: Default::default(),
            values_to_drop: Default::default(),
        }
    }
}

impl<C: Configuration> DeletedEntries<C> {
    /// # Safety
    ///
    /// The memo must be valid and safe to free when the `DeletedEntries` list is cleared or dropped.
    pub(super) unsafe fn push(&self, memo: NonNull<Memo<'_, C>>) {
        // Safety: The memo must be valid and safe to free when the `DeletedEntries` list is cleared or dropped.
        let memo =
            unsafe { std::mem::transmute::<NonNull<Memo<'_, C>>, NonNull<Memo<'static, C>>>(memo) };

        self.memos.push(SharedBox(memo));
    }

    /// # Safety
    ///
    /// The memo must be a retired memo that remains allocated until `self` is cleared or dropped.
    /// Callers must ensure no references to the memo's value exist when the queued values are dropped.
    pub(super) unsafe fn push_value_to_drop(&self, memo: NonNull<Memo<'_, C>>) {
        // Safety: The memo must remain allocated until the value is dropped.
        let memo =
            unsafe { std::mem::transmute::<NonNull<Memo<'_, C>>, NonNull<Memo<'static, C>>>(memo) };

        self.values_to_drop.push(SharedPtr(memo));
    }

    /// # Safety
    ///
    /// There must not be outstanding references to the values of retired memos.
    pub(super) unsafe fn drop_retired_values(&self) {
        while let Some(memo) = self.values_to_drop.pop() {
            // SAFETY: Guaranteed by the caller.
            unsafe { (*memo.0.as_ptr()).value = None };
        }
    }

    pub(super) fn has_retired_values(&self) -> bool {
        !self.values_to_drop.is_empty()
    }

    /// Free all deleted memos, keeping the list available for reuse.
    pub(super) fn clear(&mut self) {
        // SAFETY: `clear` is called with `&mut self` at a quiescent point.
        unsafe { self.drop_retired_values() };
        self.memos.clear();
    }
}

/// A wrapper around `NonNull` that frees the allocation when it is dropped.
struct SharedBox<T>(NonNull<T>);

/// A shared, non-owning pointer.
struct SharedPtr<T>(NonNull<T>);

impl<T> Drop for SharedBox<T> {
    fn drop(&mut self) {
        // SAFETY: Guaranteed by the caller of `DeletedEntries::push`.
        unsafe { drop(Box::from_raw(self.0.as_ptr())) };
    }
}
