use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use gns::{GnsConnection, GnsGlobal, GnsSocket, GnsUtils, IsClient, IsServer};
use gns::sys::ESteamNetworkingConnectionState;

fn tick_server(server: &GnsSocket<IsServer>) -> (Vec<GnsConnection>, Vec<GnsConnection>) {
    server.poll_callbacks();
    let mut new_connections = Vec::new();
    let mut closed_connections = Vec::new();
    server.poll_event::<100>(|event| match (event.old_state(), event.info().state()) {
        (
            ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_None,
            ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_Connecting,
        ) => {
            let connection = event.connection();
            server.accept(connection).unwrap();
            new_connections.push(connection);
        },
        (_, ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_ClosedByPeer | ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_ProblemDetectedLocally) => {
            let connection = event.connection();
            server.close_connection(connection, 0, "", false);
            closed_connections.push(connection);
        },
        _ => {},
    });
    (new_connections, closed_connections)
}

fn wait_for_server_connection(server: &GnsSocket<IsServer>) -> GnsConnection {
    loop {
        let (new_connections, closed_connections) = tick_server(server);
        if !closed_connections.is_empty() {
            panic!("Didn't expect any connection(s) to close; {closed_connections:?}");
        }
        if new_connections.len() >= 2 {
            panic!("Didn't expect multiple new connections; {new_connections:?}");
        } else if new_connections.len() == 1 {
            return new_connections[0]
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn tick_client(client: &GnsSocket<IsClient>) -> (bool, bool) {
    let mut connected = false;
    let mut closed = false;
    client.poll_callbacks();
    client.poll_event::<100>(|event| match (event.old_state(), event.info().state()) {
        (
            _,
            ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_Connected,
        ) => {
            connected = true;
        },
        (_, ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_ClosedByPeer | ESteamNetworkingConnectionState::k_ESteamNetworkingConnectionState_ProblemDetectedLocally) => {
            closed = true;
        },
        _ => {},
    });
    (connected, closed)
}

fn wait_for_client_connection(client: &GnsSocket<IsClient>) {
    loop {
        let (connected, closed) = tick_client(client);
        if closed {
            panic!("Didn't expect client to close");
        }
        if connected {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_client_closed(client: &GnsSocket<IsClient>) {
    loop {
        let (connected, closed) = tick_client(client);
        if connected {
            panic!("Didn't expect client to connect");
        }
        if closed {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn loop_client_callbacks(client: &GnsSocket<IsClient>, duration: Duration) {
    let now = Instant::now();
    loop {
        if (Instant::now() - now) > duration {
            return;
        }
        client.poll_callbacks();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() {
    let port = 50000;

    let gns_global = GnsGlobal::get().unwrap();
    let gns_utils = GnsUtils::new().unwrap();

    // 1. Create a Server socket and listen on a local port
    let server = GnsSocket::new(&gns_global, &gns_utils)
        .unwrap()
        .listen(Ipv4Addr::LOCALHOST.into(), port)
        .unwrap();

    // 2. Create the first Client socket and connect to the same local port
    let client1 = GnsSocket::new(&gns_global, &gns_utils)
        .unwrap()
        .connect(Ipv4Addr::LOCALHOST.into(), port)
        .unwrap();
    let client1_connection = wait_for_server_connection(&server);
    wait_for_client_connection(&client1);

    // 3. Create the second Client socket and connect to the same local port
    let client2 = GnsSocket::new(&gns_global, &gns_utils)
        .unwrap()
        .connect(Ipv4Addr::LOCALHOST.into(), port)
        .unwrap();
    let _client2_connection = wait_for_server_connection(&server);
    wait_for_client_connection(&client2);

    // 4.1. Close the first Client's connection from the server. NOTE: You can also close the second
    //   Client's connection, but then the repro occurs less often. Some sort of race condition
    //   where the second Client can successfully close before the AccessViolation is triggered?
    server.close_connection(client1_connection, 0, "", false);
    //server.close_connection(_client2_connection, 0, "", false);

    // 4.2. Drop the Server after closing the connection/s. This MUST be done to trigger the
    //   AccessViolation.
    drop(server);

    // 5.1. Wait for the first Client's connection to close. Also loop through the callback polling
    //   for a bit to ensure that the first Client doesn't trigger an AccessViolation at this stage.
    loop_client_callbacks(&client1, Duration::from_secs_f32(3.0));
    wait_for_client_closed(&client1);

    // 5.2. Drop the first Client after it is closed. This MUST be done to trigger the
    //   AccessViolation.
    drop(client1);

    // 6. Loop through callback polling for a bit on the second Client. It may take a few polls to
    //   trigger, depending on the timing of all of the above. 1s doesn't always trigger it, but 5s
    //   is usually enough to do so (though not _always_).
    // !!! AccessViolation occurs here !!!
    loop_client_callbacks(&client2, Duration::from_secs_f32(5.0));
}
