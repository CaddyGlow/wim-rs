//! Checked growth helpers for globally allocated collections.
use wim_format::ParseError;

pub(crate) trait FallibleCollections<T> {
    fn try_push(&mut self, value: T) -> Result<(), ParseError>;
    fn try_extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), ParseError>;
    fn try_extend_from_slice(&mut self, values: &[T]) -> Result<(), ParseError>
    where
        T: Clone;
    fn try_resize(&mut self, length: usize, value: T) -> Result<(), ParseError>
    where
        T: Clone;
}
impl<T> FallibleCollections<T> for Vec<T> {
    fn try_push(&mut self, value: T) -> Result<(), ParseError> {
        self.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        self.push(value);
        Ok(())
    }
    fn try_extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), ParseError> {
        let values = values.into_iter();
        self.try_reserve(values.size_hint().0)
            .map_err(|_| ParseError::Nomem)?;
        for value in values {
            self.try_push(value)?;
        }
        Ok(())
    }
    fn try_extend_from_slice(&mut self, values: &[T]) -> Result<(), ParseError>
    where
        T: Clone,
    {
        self.try_reserve(values.len())
            .map_err(|_| ParseError::Nomem)?;
        self.extend_from_slice(values);
        Ok(())
    }
    fn try_resize(&mut self, length: usize, value: T) -> Result<(), ParseError>
    where
        T: Clone,
    {
        self.try_reserve(length.saturating_sub(self.len()))
            .map_err(|_| ParseError::Nomem)?;
        self.resize(length, value);
        Ok(())
    }
}
pub(crate) trait FallibleMap<K, V> {
    fn try_insert_reserved(&mut self, key: K, value: V) -> Result<Option<V>, ParseError>;
}
impl<K: Eq + std::hash::Hash, V> FallibleMap<K, V> for hashbrown::HashMap<K, V> {
    fn try_insert_reserved(&mut self, key: K, value: V) -> Result<Option<V>, ParseError> {
        if !self.contains_key(&key) {
            self.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        }
        Ok(self.insert(key, value))
    }
}
pub(crate) trait FallibleSet<T> {
    fn try_insert(&mut self, value: T) -> Result<bool, ParseError>;
}
impl<T: Eq + std::hash::Hash> FallibleSet<T> for hashbrown::HashSet<T> {
    fn try_insert(&mut self, value: T) -> Result<bool, ParseError> {
        if !self.contains(&value) {
            self.try_reserve(1).map_err(|_| ParseError::Nomem)?;
        }
        Ok(self.insert(value))
    }
}
pub(crate) fn filled<T: Clone>(length: usize, value: T) -> Result<Vec<T>, ParseError> {
    let mut values = Vec::new();
    values.try_resize(length, value)?;
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::FallibleMap;

    #[test]
    fn checked_map_insert_replaces_existing_value_without_growing() {
        let mut map = hashbrown::HashMap::new();
        map.try_insert_reserved(7, "original").unwrap();
        let capacity = map.capacity();
        assert_eq!(
            map.try_insert_reserved(7, "replacement").unwrap(),
            Some("original")
        );
        assert_eq!(map.get(&7), Some(&"replacement"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.capacity(), capacity);
    }
}
