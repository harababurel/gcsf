use fuse::FileAttr;

#[derive(Clone)]
pub struct File {
    pub name: String,
    pub attr: FileAttr,
    pub pieces: Vec<String>, // filename of each piece of this file on google drive
    pub data: Option<Vec<u8>>,
}
