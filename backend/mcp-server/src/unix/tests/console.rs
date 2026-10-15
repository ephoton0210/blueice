// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn console_transport_refuses_wrong_tab_malformed_and_wrong_shape_replies() {
    for case in 0..4 {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let (tab, id, request) =
                blueice_ipc::read_client_message_with_ids(&mut server).unwrap();
            assert_eq!(tab, Some(7));
            assert_eq!(request, ClientMessage::GetConsoleMessages);
            let mut messages = blueice_ipc::console::ConsoleMessages {
                tab_id: 7,
                document_generation: 8,
                records: vec![],
                dropped_records: 0,
            };
            if case == 1 {
                messages.tab_id = 9;
            }
            if case == 2 {
                messages.document_generation = 0;
            }
            let reply = if case == 3 {
                ServerMessage::Dom("wrong shape".into())
            } else {
                ServerMessage::ConsoleMessages(messages)
            };
            blueice_ipc::write_server_message_with_ids(&mut server, Some(7), id, &reply).unwrap();
        });
        let result = CoreConnection::new(client).console_messages(Some(7));
        assert_eq!(result.is_ok(), case == 0);
        worker.join().unwrap();
    }
}
