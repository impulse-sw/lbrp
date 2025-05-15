pub(crate) fn get_from_storage(key: &str) -> Option<String> {
  web_sys::window()
    .unwrap()
    .local_storage()
    .unwrap()
    .unwrap()
    .get_item(key)
    .unwrap()
}

pub(crate) fn put_in_storage(key: &str, val: &str) {
  web_sys::window()
    .unwrap()
    .local_storage()
    .unwrap()
    .unwrap()
    .set_item(key, val)
    .unwrap();
}
