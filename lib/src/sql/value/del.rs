use crate::dbs::Options;
use crate::dbs::Runtime;
use crate::dbs::Transaction;
use crate::err::Error;
use crate::sql::array::Abolish;
use crate::sql::part::Next;
use crate::sql::part::Part;
use crate::sql::value::Value;
use async_recursion::async_recursion;
use futures::future::try_join_all;
use std::collections::HashMap;

impl Value {
	#[cfg_attr(feature = "parallel", async_recursion)]
	#[cfg_attr(not(feature = "parallel"), async_recursion(?Send))]
	pub async fn del(
		&mut self,
		ctx: &Runtime,
		opt: &Options,
		txn: &Transaction,
		path: &[Part],
	) -> Result<(), Error> {
		match path.first() {
			// Get the current path part
			Some(p) => match self {
				// Current path part is an object
				Value::Object(v) => match p {
					Part::Field(f) => match path.len() {
						1 => {
							v.remove(&f.name);
							Ok(())
						}
						_ => match v.value.get_mut(&f.name) {
							Some(v) if v.is_some() => v.del(ctx, opt, txn, path.next()).await,
							_ => Ok(()),
						},
					},
					_ => Ok(()),
				},
				// Current path part is an array
				Value::Array(v) => match p {
					Part::All => match path.len() {
						1 => {
							v.value.clear();
							Ok(())
						}
						_ => {
							let path = path.next();
							let futs = v.value.iter_mut().map(|v| v.del(ctx, opt, txn, path));
							try_join_all(futs).await?;
							Ok(())
						}
					},
					Part::First => match path.len() {
						1 => {
							if v.value.len().gt(&0) {
								v.value.remove(0);
							}
							Ok(())
						}
						_ => match v.value.first_mut() {
							Some(v) => v.del(ctx, opt, txn, path.next()).await,
							None => Ok(()),
						},
					},
					Part::Last => match path.len() {
						1 => {
							if v.value.len().gt(&0) {
								v.value.remove(v.value.len() - 1);
							}
							Ok(())
						}
						_ => match v.value.last_mut() {
							Some(v) => v.del(ctx, opt, txn, path.next()).await,
							None => Ok(()),
						},
					},
					Part::Index(i) => match path.len() {
						1 => {
							if v.value.len().gt(&i.to_usize()) {
								v.value.remove(i.to_usize());
							}
							Ok(())
						}
						_ => match v.value.get_mut(i.to_usize()) {
							Some(v) => v.del(ctx, opt, txn, path.next()).await,
							None => Ok(()),
						},
					},
					Part::Where(w) => match path.len() {
						1 => {
							let mut m = HashMap::new();
							for (i, v) in v.value.iter().enumerate() {
								if w.compute(ctx, opt, txn, Some(v)).await?.is_truthy() {
									m.insert(i, ());
								};
							}
							v.value.abolish(|i| m.contains_key(&i));
							Ok(())
						}
						_ => {
							let path = path.next();
							for v in &mut v.value {
								if w.compute(ctx, opt, txn, Some(v)).await?.is_truthy() {
									v.del(ctx, opt, txn, path).await?;
								}
							}
							Ok(())
						}
					},
					_ => match path.len() {
						1 => {
							v.value.clear();
							Ok(())
						}
						_ => {
							let futs = v.value.iter_mut().map(|v| v.del(ctx, opt, txn, path));
							try_join_all(futs).await?;
							Ok(())
						}
					},
				},
				// Ignore everything else
				_ => Ok(()),
			},
			// We are done
			None => Ok(()),
		}
	}
}

#[cfg(test)]
mod tests {

	use super::*;
	use crate::dbs::test::mock;
	use crate::sql::idiom::Idiom;
	use crate::sql::test::Parse;

	#[tokio::test]
	async fn del_none() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::default();
		let mut val = Value::parse("{ test: { other: null, something: 123 } }");
		let res = Value::parse("{ test: { other: null, something: 123 } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_reset() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test");
		let mut val = Value::parse("{ test: { other: null, something: 123 } }");
		let res = Value::parse("{ }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_basic() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something");
		let mut val = Value::parse("{ test: { other: null, something: 123 } }");
		let res = Value::parse("{ test: { other: null } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_wrong() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something.wrong");
		let mut val = Value::parse("{ test: { other: null, something: 123 } }");
		let res = Value::parse("{ test: { other: null, something: 123 } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_other() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.other.something");
		let mut val = Value::parse("{ test: { other: null, something: 123 } }");
		let res = Value::parse("{ test: { other: null, something: 123 } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[1]");
		let mut val = Value::parse("{ test: { something: [123, 456, 789] } }");
		let res = Value::parse("{ test: { something: [123, 789] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array_field() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[1].age");
		let mut val = Value::parse("{ test: { something: [{ age: 34 }, { age: 36 }] } }");
		let res = Value::parse("{ test: { something: [{ age: 34 }, { }] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array_fields() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[*].age");
		let mut val = Value::parse("{ test: { something: [{ age: 34 }, { age: 36 }] } }");
		let res = Value::parse("{ test: { something: [{ }, { }] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array_fields_flat() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[*].age");
		let mut val = Value::parse("{ test: { something: [{ age: 34 }, { age: 36 }] } }");
		let res = Value::parse("{ test: { something: [{ }, { }] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array_where_field() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[WHERE age > 35].age");
		let mut val = Value::parse("{ test: { something: [{ age: 34 }, { age: 36 }] } }");
		let res = Value::parse("{ test: { something: [{ age: 34 }, { }] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}

	#[tokio::test]
	async fn del_array_where_fields() {
		let (ctx, opt, txn) = mock().await;
		let idi = Idiom::parse("test.something[WHERE age > 35]");
		let mut val = Value::parse("{ test: { something: [{ age: 34 }, { age: 36 }] } }");
		let res = Value::parse("{ test: { something: [{ age: 34 }] } }");
		val.del(&ctx, &opt, &txn, &idi).await.unwrap();
		assert_eq!(res, val);
	}
}
