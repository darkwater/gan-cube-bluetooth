use std::error::Error;

use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter},
    platform::Manager,
};
use futures::stream::StreamExt;
use gan_cube_bluetooth::{
    bluetooth::*,
    crypt::{CryptKey, DecryptorStream},
    event::{DecoderStream, GanCubeEvent},
};
use rubikmaster::{
    cfop,
    coord::rotation_of,
    matrix::{self, PermutationMatrix},
};
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let manager = Manager::new().await?;

    // Get the first Bluetooth adapter
    let adapters = manager.adapters().await?;
    let central = adapters
        .into_iter()
        .next()
        .expect("No Bluetooth adapters found");

    // Start scanning for devices
    central.start_scan(ScanFilter::default()).await?;

    // Filter devices by name prefix
    let device = 'outer: loop {
        let peripherals = central.peripherals().await?;
        for peripheral in peripherals {
            if let Ok(Some(properties)) = peripheral.properties().await {
                if properties.local_name.iter().any(|name| {
                    name.starts_with("GAN") || name.starts_with("MG") || name.starts_with("AiCube")
                }) {
                    break 'outer Some(peripheral);
                }
            }
        }
        sleep(Duration::from_secs(1)).await;
    };

    let Some(device) = device else {
        panic!("No matching device found");
    };

    // Connect to the device
    device.connect().await?;
    println!("Connected to device: {:#?}", device);

    let mac_addr = device.address();

    // Discover services and characteristics
    device.discover_services().await?;
    let services = device.services();
    println!("Discovered services: {:#?}", services);

    // Find the characteristic we want to subscribe to
    let state_characteristic = services
        .iter()
        .flat_map(|service| service.characteristics.iter())
        .find(|c| {
            c.uuid == GAN_GEN2_STATE_CHAR_UUID
                || c.uuid == GAN_GEN3_STATE_CHAR_UUID
                || c.uuid == GAN_GEN4_STATE_CHAR_UUID
        })
        .expect("Characteristic not found");

    // Subscribe to notifications from the characteristic
    device.subscribe(state_characteristic).await?;
    println!("Subscribed to characteristic: {:?}", state_characteristic);

    // Handle notifications
    let not_stream = device.notifications().await?;
    let msg_stream =
        DecryptorStream::new(not_stream.map(|n| n.value), CryptKey::Gan, mac_addr.into_inner());
    let mut ev_stream = DecoderStream::new(msg_stream);

    let mut state = PermutationMatrix::identity();

    const TILES: &str = "rrrrrrrrrooooooooowwwwwwwwwyyyyyyyyygggggggggbbbbbbbbb";

    while let Some(event) = ev_stream.next().await {
        match event {
            Ok(GanCubeEvent::Move(m)) => {
                println!("Cube moved: {:#?}", m);

                let permut = matrix::of(rotation_of(m.command()));
                state = state * permut.inv();

                dbg!(cfop::solved(&state));
                dbg!(cfop::oll_solved(&state));
                dbg!(cfop::f2l_solved(&state));

                let tiles = state.inv().inv_perm[..]
                    .iter()
                    .map(|i| TILES.chars().nth(*i as usize).unwrap())
                    .collect::<String>();

                println!("{tiles}");
                println!("{TILES}");
            }
            Err(e) => {
                eprintln!("Error decoding event: {:#?}", e);
            }
        }
    }

    Ok(())
}
