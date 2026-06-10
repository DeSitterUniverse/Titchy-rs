use titchy_rs::{
    decompress_samples, get_by_sample_index, SampleEndian, StreamCollector, StreamDecoder,
    StreamEncoder, TitchyConfig,
};

#[test]
fn streaming_packets_round_trip_across_parameter_updates() {
    let config = TitchyConfig::new(8, 2, 3)
        .unwrap()
        .with_endian(SampleEndian::Big);
    let samples = vec![10, 11, 10, 12, 30, 31, 10, 11, 40, 41, 42];
    let mut encoder = StreamEncoder::new(config.clone(), 2).unwrap();
    let mut packets = Vec::new();

    for sample in &samples {
        if let Some(packet) = encoder.push_sample(*sample).unwrap() {
            packets.push(packet);
        }
    }
    packets.extend(encoder.finish().unwrap());

    assert!(packets.len() >= 3);
    assert!(packets[0].parameters_present());
    assert!(packets
        .iter()
        .skip(1)
        .any(|packet| packet.parameters_present()));

    let mut decoder = StreamDecoder::new(config).unwrap();
    let mut decoded = Vec::new();
    for packet in &packets {
        decoded.extend(decoder.decode_packet(packet).unwrap());
    }
    decoded.truncate(samples.len());

    assert_eq!(decoded, samples);
}

#[test]
fn packet_header_matches_paper_layout() {
    let config = TitchyConfig::new(8, 2, 100).unwrap();
    let mut encoder = StreamEncoder::new(config, 1).unwrap();

    assert!(encoder.push_sample(0x10).unwrap().is_none());
    let packet = encoder.push_sample(0x11).unwrap().unwrap();

    assert_eq!(packet.bytes()[0] & 0x80, 0x80);
    assert_eq!(packet.bytes()[0] & 0x7f, 1);
    assert_eq!(packet.chunk_count(), 1);
}

#[test]
fn decoder_rejects_first_packet_without_parameters() {
    let config = TitchyConfig::new(8, 1, 100).unwrap();
    let mut encoder = StreamEncoder::new(config.clone(), 1).unwrap();
    let first = encoder.push_sample(1).unwrap().unwrap();
    let second = encoder.push_sample(1).unwrap().unwrap();
    assert!(first.parameters_present());
    assert!(!second.parameters_present());

    let mut decoder = StreamDecoder::new(config).unwrap();
    assert!(decoder.decode_packet(&second).is_err());
}

#[test]
fn packet_chunk_target_is_not_limited_by_seven_bit_base_count() {
    let config = TitchyConfig::new(8, 1, 1000).unwrap();

    assert!(StreamEncoder::new(config, 1000).is_ok());
}

#[test]
fn collector_assembles_packets_into_random_access_container() {
    let config = TitchyConfig::new(8, 2, 3).unwrap();
    let samples = vec![1, 2, 1, 3, 4, 5, 1, 2, 8];
    let mut encoder = StreamEncoder::new(config.clone(), 2).unwrap();
    let mut packets = Vec::new();
    for &sample in &samples {
        if let Some(packet) = encoder.push_sample(sample).unwrap() {
            packets.push(packet);
        }
    }
    packets.extend(encoder.finish().unwrap());

    let mut collector = StreamCollector::new(config).unwrap();
    for packet in &packets {
        collector.ingest_packet(packet).unwrap();
    }
    let compressed = collector.finish(samples.len()).unwrap();

    assert_eq!(decompress_samples(&compressed).unwrap(), samples);
    assert_eq!(get_by_sample_index(&compressed, 7).unwrap(), 2);
    assert_eq!(compressed.to_bytes().unwrap()[4], 2);
}
