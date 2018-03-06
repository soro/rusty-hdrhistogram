use std::time::SystemTime;

pub struct HistogramMetaData {
    pub start_timestamp: Option<SystemTime>,
    pub end_timestamp: Option<SystemTime>,
    pub tag: Option<String>,
}

impl HistogramMetaData {
    pub fn new() -> HistogramMetaData {
        HistogramMetaData {
            start_timestamp: None,
            end_timestamp: None,
            tag: None,
        }
    }
    pub fn clear(&mut self) {
        self.start_timestamp = None;
        self.end_timestamp = None;
        self.tag = None;
    }
    pub fn set_start_timestamp(&mut self, time: SystemTime) {
        self.start_timestamp = Some(time);
    }
    pub fn set_end_timestamp(&mut self, time: SystemTime) {
        self.end_timestamp = Some(time);
    }
    pub fn set_tag_string(&mut self, tag_string: String) {
        self.tag = Some(tag_string);
    }
    pub fn set_start_now(&mut self) {
        self.start_timestamp = Some(SystemTime::now());
    }
    pub fn set_end_now(&mut self) {
        self.end_timestamp = Some(SystemTime::now());
    }
}
