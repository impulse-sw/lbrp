#![allow(missing_docs, dead_code)]

use impulse_ui_kit::utils::cn;
use leptos::prelude::*;

const BASE_CLASSES: &str = "file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground dark:bg-input/30 border-input h-9 w-full min-w-0 rounded-md border bg-transparent px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive";

#[component]
pub fn Input(
  #[prop(into, optional)] class: String,
  #[prop(into, optional)] r#type: String,
  #[prop(optional)] value: RwSignal<String>,
) -> impl IntoView {
  view! {
    <input
      type=r#type
      class=cn(&[BASE_CLASSES.to_string(), class])
      prop:value=value
      on:input:target=move |ev| {
        value.set(ev.target().value());
      }
    />
  }
}
