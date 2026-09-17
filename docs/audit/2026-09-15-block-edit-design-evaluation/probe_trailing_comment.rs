use confy_core::model::any_doc::AnyDocument;
use confy_core::model::document::{ConfigDocument, DocFormat, Mutation};
use confy_core::model::node::Seg;
use std::time::Instant;
fn median(mut v: Vec<f64>) -> f64 { v.sort_by(|a,b| a.partial_cmp(b).unwrap()); v[v.len()/2] }
fn run(label:&str, src:&str, path:Vec<Seg>, frag:&str){
  let mut ts=vec![];
  for _ in 0..5 { let mut d=AnyDocument::from_str_as(src,DocFormat::Toml).unwrap();
    let t=Instant::now(); d.apply(Mutation::Replace{path:path.clone(),fragment:frag.into()}).unwrap();
    ts.push(t.elapsed().as_secs_f64()*1e3); }
  println!("{label:<46}{:8.2} ms  ({} bytes)", median(ts), src.len());
}
fn main(){
  let n=5000; let mid=n/2;
  // flat: one header + 9 entries per section (bench shape, no sub-tables)
  let mut flat=String::new();
  for i in 0..n { flat.push_str(&format!("[sec{i}]\n")); for j in 0..9 { flat.push_str(&format!("k{j} = {}\n", 8000+i)); } }
  run("flat sections, leaf Replace", &flat, vec![Seg::Key(format!("sec{mid}")),Seg::Key("k1".into())], "k1 = 1  # note\n");
  // nested: adds a [secN.sub] header per section
  let mut nested=String::new();
  for i in 0..n { nested.push_str(&format!("[sec{i}]\n")); for j in 0..7 { nested.push_str(&format!("k{j} = {}\n", 8000+i)); } nested.push_str(&format!("[sec{i}.sub]\nk = {i}\n")); }
  run("nested sub-sections, leaf Replace", &nested, vec![Seg::Key(format!("sec{mid}")),Seg::Key("k1".into())], "k1 = 1  # note\n");
  // same nested doc, whole-document Replace
  let mut ts=vec![];
  for _ in 0..5 { let mut d=AnyDocument::from_str_as(&nested,DocFormat::Toml).unwrap();
    let t=Instant::now(); let txt=d.serialize(); d.apply(Mutation::Replace{path:vec![],fragment:txt}).unwrap();
    ts.push(t.elapsed().as_secs_f64()*1e3); }
  println!("{:<46}{:8.2} ms", "nested doc, whole-document Replace", median(ts));
}
