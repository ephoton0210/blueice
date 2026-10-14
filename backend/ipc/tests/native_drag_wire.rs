// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ipc::input::{
    NativeDragAction, NativeDragModifiers, NativeDragMouse, NativeDragOperation, NativeDragState,
    PointerContext, PointerPoint,
};
use blueice_ipc::{ClientMessage, ServerMessage};

#[test]
fn native_drag_wire_preserves_fractional_mouse_coordinates_exactly() {
    // These fractional coordinates were captured from a real macOS drag.
    // Changing even one bit makes the native owner reject its echoed ACK.
    for value in [14.888885498046875, 180.88888549804688, 0.1, -0.1] {
        let point = PointerPoint { x: 28.0, y: value };
        let mouse = NativeDragMouse {
            client_point: point,
            screen_point: PointerPoint { x: 68.0, y: value },
            modifiers: NativeDragModifiers::default(),
            buttons: 1,
        };
        assert!(mouse.valid());
        let command = ClientMessage::NativeDrag {
            context: PointerContext {
                version: 1,
                frame_source: 2,
                document_generation: 3,
            },
            frame_generation: 4,
            action: NativeDragAction::SourceMove { drag_id: 5, point },
            mouse: Some(mouse),
        };
        let mut bytes = Vec::new();
        blueice_ipc::write_client_message_with_ids(&mut bytes, Some(1), Some(6), &command).unwrap();
        let (tab, request, decoded) =
            blueice_ipc::read_client_message_with_ids(&mut bytes.as_slice()).unwrap();
        assert_eq!((tab, request), (Some(1), Some(6)));
        let ClientMessage::NativeDrag {
            mouse: Some(observed),
            action:
                NativeDragAction::SourceMove {
                    point: observed_point,
                    ..
                },
            ..
        } = decoded
        else {
            panic!("Expected the owned native drag command");
        };
        assert_eq!(observed_point.y.to_bits(), value.to_bits());
        assert_eq!(
            observed.client_point.y.to_bits(),
            value.to_bits(),
            "Native drag client coordinate must preserve its exact f64 bits"
        );
        assert_eq!(observed.screen_point.y.to_bits(), value.to_bits());
        let reply = ServerMessage::NativeDragState(NativeDragState {
            version: 1,
            drag_id: 5,
            frame_source: 2,
            document_generation: 3,
            frame_generation: 4,
            active: true,
            operation: NativeDragOperation::Copy,
            allowed_operations: vec![NativeDragOperation::Copy],
            mouse: Some(observed),
            items: None,
            image: None,
        });
        bytes.clear();
        blueice_ipc::write_server_message_with_ids(&mut bytes, tab, request, &reply).unwrap();
        let (reply_tab, reply_request, decoded) =
            blueice_ipc::read_server_message_with_ids(&mut bytes.as_slice()).unwrap();
        assert_eq!((reply_tab, reply_request), (tab, request));
        let ServerMessage::NativeDragState(state) = decoded else {
            panic!("Expected the owned native drag acknowledgement");
        };
        let echoed = state.mouse.unwrap();
        assert_eq!(
            echoed.client_point.y.to_bits(),
            value.to_bits(),
            "Native drag ACK must retain the original mouse coordinate bits"
        );
        assert_eq!(echoed.screen_point.y.to_bits(), value.to_bits());
        assert_eq!(echoed, mouse);
    }
}
