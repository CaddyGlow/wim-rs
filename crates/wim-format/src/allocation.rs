//! Fallible growth helpers for ordinary allocated collections.
use crate::ParseError;
use alloc::{collections::TryReserveError, vec::Vec};
use core::hash::Hash;
use hashbrown::{HashMap, HashSet};

pub(crate) trait VecTryExt<T> {
    fn try_push(&mut self, value: T) -> Result<(), TryReserveError>;
    fn try_extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), TryReserveError>;
    fn try_extend_from_slice(&mut self, values: &[T]) -> Result<(), TryReserveError>
    where
        T: Clone;
}
impl<T> VecTryExt<T> for Vec<T> {
    fn try_push(&mut self, value: T) -> Result<(), TryReserveError> {
        self.try_reserve(1)?;
        self.push(value);
        Ok(())
    }
    fn try_extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), TryReserveError> {
        let values = values.into_iter();
        self.try_reserve(values.size_hint().0)?;
        for value in values {
            self.try_push(value)?;
        }
        Ok(())
    }
    fn try_extend_from_slice(&mut self, values: &[T]) -> Result<(), TryReserveError>
    where
        T: Clone,
    {
        self.try_reserve(values.len())?;
        self.extend_from_slice(values);
        Ok(())
    }
}
pub(crate) trait MapTryExt<K, V> {
    fn try_insert_checked(
        &mut self,
        key: K,
        value: V,
    ) -> Result<Option<V>, hashbrown::TryReserveError>;
}
impl<K: Eq + Hash, V> MapTryExt<K, V> for HashMap<K, V> {
    fn try_insert_checked(
        &mut self,
        key: K,
        value: V,
    ) -> Result<Option<V>, hashbrown::TryReserveError> {
        if !self.contains_key(&key) {
            self.try_reserve(1)?;
        }
        Ok(self.insert(key, value))
    }
}
pub(crate) trait SetTryExt<K> {
    fn try_insert_checked(&mut self, key: K) -> Result<bool, hashbrown::TryReserveError>;
}
impl<K: Eq + Hash> SetTryExt<K> for HashSet<K> {
    fn try_insert_checked(&mut self, key: K) -> Result<bool, hashbrown::TryReserveError> {
        if !self.contains(&key) {
            self.try_reserve(1)?;
        }
        Ok(self.insert(key))
    }
}
pub(crate) fn map<K: Eq + Hash, V>(capacity: usize) -> Result<HashMap<K, V>, ParseError> {
    let mut map = HashMap::new();
    map.try_reserve(capacity).map_err(|_| ParseError::Nomem)?;
    Ok(map)
}
