//! Komodo added impl for Peer Statistics (for getpeerinfo rpc)

use std::{net::SocketAddr, collections::HashMap, time::{SystemTime, Instant}, sync::{Arc, Mutex}};

use crate::{meta_addr::{MetaAddr, MetaAddrChange}, PeerAddrState};
use zebra_chain::parameters::Network;
use thiserror::Error;

use crate::{
    Config,
};

use metrics::{Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, Recorder, Unit, KeyName};

#[allow(unused)]
#[derive(Debug)]
enum MetricOperation {
    IncrementCounter(u64),
    SetCounter(u64),
    IncrementGauge(f64),
    DecrementGauge(f64),
    SetGauge(f64),
    RecordHistogram(f64),
}

/// Event with metrics data to pass into peer stats
#[derive(Debug)]
pub struct MetricEvent(Key, MetricOperation);

/// Error to notify peer stats forwarder closed
#[derive(Copy, Clone, Debug, Error, Eq, PartialEq, Hash)]
#[error("peer stats metrics data forwarder is closed")]
pub struct PeerStatForwardSenderClosed;


/// Peer stat data collected from network requests 
#[allow(missing_docs)]
#[derive(Copy, Debug, Clone)]
pub struct PeerNetStat {
    /// See analoguous fields in the GetPeerInfo struct    
    pub connection_time: SystemTime,
    pub last_attempt_time: SystemTime,
    pub out_bytes_total: u64, 
    pub in_bytes_total: u64, 
    pub out_messages: u64, 
    pub in_messages: u64, 
    pub last_ping_time: Option<Instant>,
    pub last_pong_time: Option<Instant>,
    pub in_flight: u64,

    /// initially false, is set to true if metrics data were ever received
    pub metrics_used: bool,

}

impl PeerNetStat {
    fn new() -> Self {
        let now = SystemTime::now();
        Self {
            connection_time: now,
            last_attempt_time: now,
            out_bytes_total: 0,
            in_bytes_total: 0,
            out_messages: 0,
            in_messages: 0,
            last_ping_time: None,
            last_pong_time: None,
            in_flight: 0,
            metrics_used: false,
        }
    } 
}

/// Stat data for one peer
#[derive(Copy, Debug, Clone)]
pub struct PeerStatData {

    /// peer address with its network status
    pub meta_addr: MetaAddr,

    /// peer network stat values
    pub net_stat: PeerNetStat,

    /// inbound or outbound connection
    pub is_inbound: bool,
}

/// Live peers' statistics list
#[derive(Debug, Clone)]
pub struct PeerStats {
    /// Connected peer addresses,
    by_addr: HashMap<SocketAddr, PeerStatData>,

    /// The configured Zcash network.
    network: Network,
}

impl PeerStats {
    
    /// create holding peers statistics object
    pub fn new(config: &Config) -> 
        Arc<Mutex<PeerStats>>
    {
        let peer_stats = PeerStats {
            by_addr: HashMap::new(),
            network: config.network,
        };
        let peer_stats = Arc::new(std::sync::Mutex::new(peer_stats));

        // I decided not to use channel to forward metrics data to PeerStats (for diminishing possible metrics processing delay)
        // With the channel use there was an issue with shutdown:
        // To shutdown properly we need to stop the worker thread which normally is waiting for the worker_metrics_rx.blocking_recv() call
        // to stop that call we must drop all sending part of the channel. 
        // And for that metrics_forwarder (which holds the worker_metrics_tx clone) must be dropped too
        // But this does not happen in the metrics code where the forwarder is passed with metrics::set_boxed_recorder()
        // so the worker thread never ends gracefully and just cancelled with some delay on shutdown, this does not look good.
        // I think we are doing metrics processing pretty fast in update_with_metrics() fn and may not use a channel
        
        if !config.dont_use_metrics_for_getpeerinfo {
            let metrics_forwarder = Arc::new(Box::new(ForwardRecorder{ peer_stats: peer_stats.clone() }));
            if metrics::set_boxed_recorder(metrics_forwarder.as_ref().to_owned()).is_err() {
                info!("could not set komodo peer stat metrics recorder, probably used by other monitoring software")
            };
        }

        peer_stats
    }

    /// update peer stats with address change
    #[allow(clippy::unwrap_in_result)]
    pub fn update(&mut self, change: MetaAddrChange) -> Option<PeerStatData> {
        let previous = self.by_addr.get(&change.addr());

        let previous_meta_addr = if let Some(previous) = previous { Some(previous.meta_addr) } else { None };
        let updated_meta_addr = change.apply_to_meta_addr(previous_meta_addr);

        trace!(
            ?change,
            ?updated_meta_addr,
            ?previous,
            total_peers = self.by_addr.len(),
            "calculated updated PeerStats entry",
        );

        if let Some(updated_meta_addr) = updated_meta_addr {

            // add only active peers to stat: 
            if updated_meta_addr.last_connection_state == PeerAddrState::Responded ||
                updated_meta_addr.last_connection_state == PeerAddrState::AttemptPending {

                let mut updated = match previous {
                    Some(updated) => updated.to_owned(),
                    None => {
                        PeerStatData { 
                            meta_addr: updated_meta_addr, 
                            net_stat: PeerNetStat::new(),
                            is_inbound: !updated_meta_addr.address_is_valid_for_outbound(self.network),
                        }
                    },
                };

                if updated_meta_addr.last_connection_state == PeerAddrState::AttemptPending {
                    updated.net_stat.last_attempt_time = SystemTime::now();
                }

                self.by_addr.insert(
                    updated_meta_addr.addr, 
                    updated,
                );

                debug!(
                    ?change,
                    ?updated_meta_addr,
                    ?updated,
                    total_peers = self.by_addr.len(),
                    "updated PeerStats entry",
                );
                return Some(updated);
            } else {
                self.by_addr.remove(&change.addr()); // remove not live addr
            }
        }

        None
    }

    /// Update peer stats with metrics data change
    /// Returns updated PeerStatData or none if nothing was updated
    #[allow(clippy::unwrap_in_result)]
    pub fn update_with_metrics(&mut self, change: MetricEvent) -> Option<PeerStatData> {

        if let Some(addr) = change.0.labels().find(|l| l.key() == "addr") {
            if let Ok(addr) = addr.value().parse() {
                if let Some(updated) = self.by_addr.get_mut(&addr) {

                    match (change.0.name(), change.1) {

                        ("zcash.net.in.bytes.total", MetricOperation::IncrementCounter(v)) => updated.net_stat.in_bytes_total += v,
                        ("zcash.net.out.bytes.total", MetricOperation::IncrementCounter(v)) => updated.net_stat.out_bytes_total += v,
                        ("zcash.net.in.messages", MetricOperation::IncrementCounter(v)) => {
                            if let Some(_pong) = change.0.labels().find(|l| l.key() == "command" && l.value() == "pong" ) {
                                // get pong time
                                updated.net_stat.last_pong_time = Some(Instant::now());
                            } 
                            updated.net_stat.in_messages += v;
                        },
                        ("zcash.net.out.messages", MetricOperation::IncrementCounter(v)) => {
                            if let Some(_ping) = change.0.labels().find(|l| l.key() == "command" && l.value() == "ping") {
                                // get ping time
                                updated.net_stat.last_ping_time = Some(Instant::now());
                            }                            
                            updated.net_stat.out_messages += v;
                        },
                        ("zebra.net.out.requests", MetricOperation::IncrementCounter(v)) => {
                            if let Some(_pong) = change.0.labels().find(|l| l.key() == "command" && l.value() == "BlocksByHash" ) {
                                updated.net_stat.in_flight = updated.net_stat.in_flight.saturating_add(v); // increment inflight
                            } 
                        },
                        ("zebra.net.in.responses", MetricOperation::IncrementCounter(v)) => {
                            if let Some(_pong) = change.0.labels().find(|l| l.key() == "command" && l.value() == "Blocks" ) {
                                updated.net_stat.in_flight = updated.net_stat.in_flight.saturating_sub(v); // decrement inflight
                            } 
                        },
                        (_,_) => {},
                    }
                    updated.net_stat.metrics_used = true;
                    return Some(*updated);
                }
            }
        }
        None
    }

    /// Return an iterator over peers stats.
    ///
    /// Returns peers stats.
    pub fn peers(&'_ self) -> impl Iterator<Item = PeerStatData> + '_ {
        self.by_addr.values().cloned()
    }

}

/// handle of metrics data forwarder to peer stat 
struct ForwardHandle {
    /// metrics data key
    key: Key,

    /// peer stats object to update it with metrics data
    peer_stats: Arc<Mutex<PeerStats>>,
}

#[allow(unused)]
impl CounterFn for ForwardHandle {
    fn increment(&self, value: u64) {
        self.peer_stats.lock().unwrap().update_with_metrics(MetricEvent(self.key.clone(), MetricOperation::IncrementCounter(value)));
    }

    fn absolute(&self, value: u64) {
        self.peer_stats.lock().unwrap().update_with_metrics(MetricEvent(self.key.clone(), MetricOperation::SetCounter(value)));
    }
}

#[allow(unused)]
impl GaugeFn for ForwardHandle {
    fn increment(&self, value: f64) {
        self.peer_stats.lock().unwrap().update_with_metrics(MetricEvent(self.key.clone(), MetricOperation::IncrementGauge(value)));
    }

    fn decrement(&self, value: f64) {
        self.peer_stats.lock().unwrap().update_with_metrics(MetricEvent(self.key.clone(), MetricOperation::DecrementGauge(value)));
    }

    fn set(&self, value: f64) {
        self.peer_stats.lock().unwrap().update_with_metrics(MetricEvent(self.key.clone(), MetricOperation::SetGauge(value)));
    }
}

#[allow(unused)]
impl HistogramFn for ForwardHandle {
    fn record(&self, value: f64) {
        // we are not interested in this for now
    }
}

/// forwarder of metrics data to peer stats
#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ForwardRecorder {
    peer_stats: Arc<Mutex<PeerStats>>,
}

impl Recorder for ForwardRecorder {
    fn describe_counter(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {}

    fn describe_gauge(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {}

    fn describe_histogram(&self, _key_name: KeyName, _unit: Option<Unit>, _description: &'static str) {}

    fn register_counter(&self, key: &Key) -> Counter {
        Counter::from_arc(Arc::new(ForwardHandle{ peer_stats: self.peer_stats.clone(), key: key.clone() }))
    }

    fn register_gauge(&self, key: &Key) -> Gauge {
        Gauge::from_arc(Arc::new(ForwardHandle{ peer_stats: self.peer_stats.clone(), key: key.clone() }))
    }

    fn register_histogram(&self, key: &Key) -> Histogram {
        Histogram::from_arc(Arc::new(ForwardHandle{ peer_stats: self.peer_stats.clone(), key: key.clone() }))
    }
}
