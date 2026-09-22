// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS's client half of the long-lived core script protocol.

use crate::interpreter::{ClassListOperation, DomHost};
use blueice_ipc::script::{
    ScriptClassListOperation, ScriptCommand, ScriptReply, ScriptRequest, ScriptTurnOutcome,
    read_script_command, read_script_reply, write_script_request,
};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::rc::Rc;

/// One script realm's core connection, fixed to one tab. A page navigation
/// replaces the realm rather than changing this binding's `tab_id`, avoiding
/// accidental cross-tab DOM calls from a stale closure.
#[derive(Debug)]
pub struct ScriptDomClient<S> {
    stream: Rc<RefCell<S>>,
    tab_id: u64,
}

impl<S> Clone for ScriptDomClient<S> {
    fn clone(&self) -> Self {
        Self {
            stream: Rc::clone(&self.stream),
            tab_id: self.tab_id,
        }
    }
}

impl ScriptDomClient<UnixStream> {
    pub fn connect(path: &Path, tab_id: u64) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        Self::handshake(stream, tab_id)
    }
}

impl<S: Read + Write> ScriptDomClient<S> {
    pub fn handshake(mut stream: S, tab_id: u64) -> io::Result<Self> {
        write_script_request(&mut stream, &ScriptRequest::Hello)?;
        match read_script_reply(&mut stream)? {
            ScriptReply::HelloAck => Ok(Self {
                stream: Rc::new(RefCell::new(stream)),
                tab_id,
            }),
            ScriptReply::Error { message } => Err(io::Error::other(message)),
            reply => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected BlueJS hello reply: {reply:?}"),
            )),
        }
    }

    /// Retains the same transport while binding future DOM requests to a
    /// different tab realm. The daemon uses this to keep one OS process and
    /// one socket while isolating each tab's VM globals.
    pub fn for_tab(&self, tab_id: u64) -> Self {
        Self {
            stream: Rc::clone(&self.stream),
            tab_id,
        }
    }

    fn request(&mut self, request: ScriptRequest) -> Result<ScriptReply, String> {
        let mut stream = self.stream.borrow_mut();
        write_script_request(&mut *stream, &request).map_err(|error| error.to_string())?;
        match read_script_reply(&mut *stream).map_err(|error| error.to_string())? {
            ScriptReply::Error { message } => Err(message),
            reply => Ok(reply),
        }
    }

    /// Waits for the next core-issued control command. It must be called only
    /// while no VM is actively servicing a DOM request on this connection.
    pub fn read_command(&mut self) -> Result<ScriptCommand, String> {
        read_script_command(&mut *self.stream.borrow_mut()).map_err(|error| error.to_string())
    }

    /// Marks an Execute turn complete only after `Vm::evaluate` has returned;
    /// core replies with `Ack` after it has observed the barrier.
    pub fn complete(
        &mut self,
        tab_id: u64,
        error: Option<String>,
        outcome: ScriptTurnOutcome,
    ) -> Result<(), String> {
        match self.request(ScriptRequest::Complete {
            tab_id,
            error,
            outcome,
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected script completion reply: {reply:?}")),
        }
    }
}

impl<S: Read + Write + std::fmt::Debug> DomHost for ScriptDomClient<S> {
    fn get_element_by_id(&mut self, id: &str) -> Result<Option<u64>, String> {
        match self.request(ScriptRequest::GetElementById {
            tab_id: self.tab_id,
            id: id.to_string(),
        })? {
            ScriptReply::Node { node } => Ok(node),
            reply => Err(format!("unexpected getElementById reply: {reply:?}")),
        }
    }

    fn query_selector(&mut self, selector: &str) -> Result<Option<u64>, String> {
        match self.request(ScriptRequest::QuerySelector {
            tab_id: self.tab_id,
            selector: selector.to_string(),
        })? {
            ScriptReply::Node { node } => Ok(node),
            reply => Err(format!("unexpected querySelector reply: {reply:?}")),
        }
    }

    fn query_selector_all(&mut self, selector: &str) -> Result<Vec<u64>, String> {
        match self.request(ScriptRequest::QuerySelectorAll {
            tab_id: self.tab_id,
            selector: selector.to_string(),
        })? {
            ScriptReply::Nodes { nodes } => Ok(nodes),
            reply => Err(format!("unexpected querySelectorAll reply: {reply:?}")),
        }
    }

    fn create_element(&mut self, tag_name: &str) -> Result<u64, String> {
        match self.request(ScriptRequest::CreateElement {
            tab_id: self.tab_id,
            tag_name: tag_name.to_string(),
        })? {
            ScriptReply::NodeCreated { node } => Ok(node),
            reply => Err(format!("unexpected createElement reply: {reply:?}")),
        }
    }

    fn create_text_node(&mut self, data: &str) -> Result<u64, String> {
        match self.request(ScriptRequest::CreateTextNode {
            tab_id: self.tab_id,
            data: data.to_string(),
        })? {
            ScriptReply::NodeCreated { node } => Ok(node),
            reply => Err(format!("unexpected createTextNode reply: {reply:?}")),
        }
    }

    fn append_child(&mut self, parent: u64, child: u64) -> Result<(), String> {
        match self.request(ScriptRequest::AppendChild {
            tab_id: self.tab_id,
            parent,
            child,
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected appendChild reply: {reply:?}")),
        }
    }

    fn insert_before(
        &mut self,
        parent: u64,
        child: u64,
        reference: Option<u64>,
    ) -> Result<(), String> {
        match self.request(ScriptRequest::InsertBefore {
            tab_id: self.tab_id,
            parent,
            child,
            reference,
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected insertBefore reply: {reply:?}")),
        }
    }

    fn remove_child(&mut self, parent: u64, child: u64) -> Result<(), String> {
        match self.request(ScriptRequest::RemoveChild {
            tab_id: self.tab_id,
            parent,
            child,
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected removeChild reply: {reply:?}")),
        }
    }

    fn remove(&mut self, node: u64) -> Result<(), String> {
        match self.request(ScriptRequest::Remove {
            tab_id: self.tab_id,
            node,
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected remove reply: {reply:?}")),
        }
    }

    fn get_text_content(&mut self, node: u64) -> Result<String, String> {
        match self.request(ScriptRequest::GetTextContent {
            tab_id: self.tab_id,
            node,
        })? {
            ScriptReply::Text { value } => Ok(value),
            reply => Err(format!("unexpected textContent reply: {reply:?}")),
        }
    }

    fn set_text_content(&mut self, node: u64, value: &str) -> Result<(), String> {
        match self.request(ScriptRequest::SetTextContent {
            tab_id: self.tab_id,
            node,
            value: value.to_string(),
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected set textContent reply: {reply:?}")),
        }
    }

    fn get_attribute(&mut self, node: u64, name: &str) -> Result<Option<String>, String> {
        match self.request(ScriptRequest::GetAttribute {
            tab_id: self.tab_id,
            node,
            name: name.to_string(),
        })? {
            ScriptReply::Attribute { value } => Ok(value),
            reply => Err(format!("unexpected getAttribute reply: {reply:?}")),
        }
    }

    fn set_attribute(&mut self, node: u64, name: &str, value: &str) -> Result<(), String> {
        match self.request(ScriptRequest::SetAttribute {
            tab_id: self.tab_id,
            node,
            name: name.to_string(),
            value: value.to_string(),
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected setAttribute reply: {reply:?}")),
        }
    }

    fn remove_attribute(&mut self, node: u64, name: &str) -> Result<(), String> {
        match self.request(ScriptRequest::RemoveAttribute {
            tab_id: self.tab_id,
            node,
            name: name.to_string(),
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected removeAttribute reply: {reply:?}")),
        }
    }

    fn inner_html(&mut self, node: u64) -> Result<String, String> {
        match self.request(ScriptRequest::GetInnerHtml {
            tab_id: self.tab_id,
            node,
        })? {
            ScriptReply::Text { value } => Ok(value),
            reply => Err(format!("unexpected innerHTML reply: {reply:?}")),
        }
    }

    fn get_style_property(&mut self, node: u64, property: &str) -> Result<String, String> {
        match self.request(ScriptRequest::GetStyleProperty {
            tab_id: self.tab_id,
            node,
            property: property.to_string(),
        })? {
            ScriptReply::Text { value } => Ok(value),
            reply => Err(format!("unexpected style getter reply: {reply:?}")),
        }
    }

    fn set_style_property(&mut self, node: u64, property: &str, value: &str) -> Result<(), String> {
        match self.request(ScriptRequest::SetStyleProperty {
            tab_id: self.tab_id,
            node,
            property: property.to_string(),
            value: value.to_string(),
        })? {
            ScriptReply::Ack => Ok(()),
            reply => Err(format!("unexpected style setter reply: {reply:?}")),
        }
    }

    fn class_list(
        &mut self,
        node: u64,
        operation: ClassListOperation,
        class_name: &str,
    ) -> Result<bool, String> {
        let operation = match operation {
            ClassListOperation::Add => ScriptClassListOperation::Add,
            ClassListOperation::Remove => ScriptClassListOperation::Remove,
            ClassListOperation::Toggle => ScriptClassListOperation::Toggle,
            ClassListOperation::Contains => ScriptClassListOperation::Contains,
        };
        match self.request(ScriptRequest::ClassList {
            tab_id: self.tab_id,
            node,
            operation,
            class_name: class_name.to_string(),
        })? {
            ScriptReply::Bool { value } => Ok(value),
            reply => Err(format!("unexpected classList reply: {reply:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::script::{read_script_request, write_script_reply};
    use std::os::unix::net::UnixStream;
    use std::thread;

    #[test]
    fn client_handshakes_then_preserves_its_tab_on_every_dom_request() {
        let (client, mut core) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || {
            assert_eq!(
                read_script_request(&mut core).unwrap(),
                ScriptRequest::Hello
            );
            write_script_reply(&mut core, &ScriptReply::HelloAck).unwrap();
            assert_eq!(
                read_script_request(&mut core).unwrap(),
                ScriptRequest::GetElementById {
                    tab_id: 7,
                    id: "target".to_string()
                }
            );
            write_script_reply(&mut core, &ScriptReply::Node { node: Some(42) }).unwrap();
        });

        let mut client = ScriptDomClient::handshake(client, 7).unwrap();
        assert_eq!(client.get_element_by_id("target").unwrap(), Some(42));
        server.join().unwrap();
    }
}
