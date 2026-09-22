use async_trait::async_trait;
use std::path::Path;
use std::time::UNIX_EPOCH;
use super::{FileContent, FileEntry, FileOps, WriteOutcome};
use roc_desk_core::error::AppError;
#[derive(Debug, Default, Clone, Copy)] pub struct LocalFileOps;
fn mtime(p:&Path)->i64{std::fs::metadata(p).and_then(|m|m.modified()).ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_secs() as i64).unwrap_or(0)}
fn err(p:&Path,e:std::io::Error)->AppError{AppError::Internal(format!("{}: {}",p.display(),e))}
#[async_trait] impl FileOps for LocalFileOps{
 async fn list_dir(&self,path:&str)->Result<Vec<FileEntry>,AppError>{let p=Path::new(path);let mut out=Vec::new();for i in std::fs::read_dir(p).map_err(|e|err(p,e))?{let i=i.map_err(|e|err(p,e))?;let q=i.path();let m=i.metadata().map_err(|e|err(&q,e))?;out.push(FileEntry{name:i.file_name().to_string_lossy().into_owned(),path:q.to_string_lossy().into_owned(),is_dir:m.is_dir(),size:m.is_file().then_some(m.len()),modified:Some(mtime(&q))});}out.sort_by_key(|e|(!e.is_dir,e.name.to_lowercase()));Ok(out)}
 async fn read_file(&self,path:&str)->Result<FileContent,AppError>{let p=Path::new(path);let b=std::fs::read(p).map_err(|e|err(p,e))?;Ok(FileContent{text:String::from_utf8_lossy(&b).into_owned(),encoding:"utf-8".into(),mtime:mtime(p),total_size:b.len() as u64,truncated:false})}
 async fn write_file(&self,path:&str,text:&str,expected:Option<i64>)->Result<WriteOutcome,AppError>{let p=Path::new(path);let cur=mtime(p);if let Some(x)=expected{if p.exists()&&x!=cur{return Ok(WriteOutcome::Conflict{current_mtime:cur,current_preview:self.read_file(path).await.map(|v|v.text.chars().take(512).collect()).unwrap_or_default()})}}if let Some(d)=p.parent(){std::fs::create_dir_all(d).map_err(|e|err(d,e))?}std::fs::write(p,text).map_err(|e|err(p,e))?;Ok(WriteOutcome::Written{mtime:mtime(p)})}
 async fn delete(&self,path:&str,is_dir:bool)->Result<(),AppError>{let p=Path::new(path);if is_dir{std::fs::remove_dir_all(p)}else{std::fs::remove_file(p)}.map_err(|e|err(p,e))}
 async fn create_dir(&self,path:&str)->Result<(),AppError>{let p=Path::new(path);std::fs::create_dir_all(p).map_err(|e|err(p,e))}
}
