#![allow(missing_docs, dead_code)]

//! Usage:
//!
//! icondata = { version = "0.5", default-features = false }

use impulse_ui_kit::utils::cn;
use leptos::prelude::*;

#[component]
pub fn Icon(#[prop(into)] icon: icondata::Icon, #[prop(optional, into)] class: String) -> impl IntoView {
  view! {
    <svg
      class=cn(&["inline-block", class.as_str()])
      x=icon.x
      y=icon.y
      viewBox=icon.view_box
      stroke-linecap=icon.stroke_linecap
      stroke-linejoin=icon.stroke_linejoin
      stroke-width=icon.stroke_width
      stroke=icon.stroke
      fill=icon.fill.unwrap_or("currentColor")
      inner_html=icon.data
    />
  }
}
