use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseKind { Mysql, Tdsql, Postgres, Opengauss, SqlServer, Oracle }
impl DatabaseKind { pub const fn default_port(self) -> u16 { match self { Self::Mysql=>3306, Self::Tdsql=>15300, Self::Postgres|Self::Opengauss=>5432, Self::SqlServer=>1433, Self::Oracle=>1521 } } }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataSourceProfile { pub id:String, pub name:String, pub kind:DatabaseKind, pub host:String, pub port:u16, pub database:String, pub username:Option<String>, pub credential_ref:Option<String>, pub readonly:bool, pub ssl_required:bool }
impl DataSourceProfile { pub fn new(id:impl Into<String>,name:impl Into<String>,kind:DatabaseKind,host:impl Into<String>,database:impl Into<String>)->Self { let port=kind.default_port(); Self{id:id.into(),name:name.into(),kind,host:host.into(),port,database:database.into(),username:None,credential_ref:None,readonly:false,ssl_required:false} } }
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryColumn { pub name:String, pub data_type:String }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryCell { pub text:Option<String>, pub is_binary:bool }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryResult { pub columns:Vec<QueryColumn>, pub rows:Vec<Vec<QueryCell>>, pub affected_rows:Option<u64>, pub truncated:bool }
impl QueryResult { pub fn empty()->Self{Self{columns:Vec::new(),rows:Vec::new(),affected_rows:None,truncated:false}} pub fn row_count(&self)->usize{self.rows.len()} }
