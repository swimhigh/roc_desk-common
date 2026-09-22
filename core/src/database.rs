use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseKind { Mysql, Tdsql, Postgres, Opengauss, SqlServer, Oracle }
impl DatabaseKind { pub const fn default_port(self) -> u16 { match self { Self::Mysql=>3306, Self::Tdsql=>15300, Self::Postgres|Self::Opengauss=>5432, Self::SqlServer=>1433, Self::Oracle=>1521 } } }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataSourceProfile { pub id:String, pub name:String, pub kind:DatabaseKind, pub host:String, pub port:u16, pub database:String, pub username:Option<String>, pub credential_ref:Option<String>, pub readonly:bool, pub ssl_required:bool }
impl DataSourceProfile { pub fn new(id:impl Into<String>,name:impl Into<String>,kind:DatabaseKind,host:impl Into<String>,database:impl Into<String>)->Self { let port=kind.default_port(); Self{id:id.into(),name:name.into(),kind,host:host.into(),port,database:database.into(),username:None,credential_ref:None,readonly:false,ssl_required:false} } }
