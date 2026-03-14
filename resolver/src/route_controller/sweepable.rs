use std::collections::{BTreeMap, BTreeSet, HashMap};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};

#[derive(Eq, PartialEq, PartialOrd, Ord, Clone, Copy)]
enum Boundary<T: Ord> {
    Value(T),
    Infinity,
}

pub fn resolve_subnets<T>(subnets: Vec<(T::Net, u32, i64)>) -> Vec<(IpNet, u32)>
where
    T: SweepableIp,
    T::Net: Copy + Ord + Send + Sync,
{
    if subnets.is_empty() { return vec![]; }

    #[derive(Eq, PartialEq, PartialOrd, Ord)]
    struct Event<T: Ord> {
        ip: Boundary<T>,
        is_start: bool,
        priority: i64,
        prefix_len: u8,
        mark: u32,
    }

    let mut events = Vec::with_capacity(subnets.len() * 2);
    let mut points = BTreeSet::new();
    for (net, mark, priority) in subnets {
        let s = T::network(net);
        let e = T::broadcast(net);

        let start = Boundary::Value(s);
        let end = if e == T::max_val() { Boundary::Infinity } else { Boundary::Value(T::add_one(e)) };

        events.push(Event { ip: start, is_start: true, priority, prefix_len: T::prefix_len(net), mark });
        events.push(Event { ip: end, is_start: false, priority, prefix_len: T::prefix_len(net), mark });
        points.insert(start);
        points.insert(end);
    }

    let sorted_points: Vec<_> = points.into_iter().collect();
    events.sort();

    let mut active = BTreeMap::<(i64, u8, u32), usize>::new();
    let mut result_intervals = Vec::new();
    let mut event_idx = 0;

    for i in 0..sorted_points.len() - 1 {
        let start = sorted_points[i];
        let next = sorted_points[i+1];

        while event_idx < events.len() && events[event_idx].ip == start {
            let e = &events[event_idx];
            let key = (e.priority, e.prefix_len, e.mark);
            if e.is_start {
                *active.entry(key).or_insert(0) += 1;
            } else {
                if let Some(c) = active.get_mut(&key) {
                    *c -= 1;
                    if *c == 0 { active.remove(&key); }
                }
            }
            event_idx += 1;
        }

        if let Some((_, _, mark)) = active.keys().last() {
            let start_val = match start { Boundary::Value(v) => v, Boundary::Infinity => unreachable!() };
            let end_val = match next { Boundary::Value(v) => T::sub_one(v), Boundary::Infinity => T::max_val() };
            result_intervals.push((start_val, end_val, *mark));
        }
    }

    let mut by_mark: HashMap<u32, Vec<IpNet>> = HashMap::new();
    for (start, end, mark) in result_intervals {
        for net in T::range_to_cidrs(start, end) {
            by_mark.entry(mark).or_default().push(T::get_ipnet(net));
        }
    }

    let mut final_res = Vec::new();
    for (mark, nets) in by_mark {
        for aggregated in IpNet::aggregate(&nets) {
            final_res.push((aggregated, mark));
        }
    }
    final_res
}

pub trait SweepableIp: Copy + Ord + Eq + Send + Sync + 'static {
    type Net: Copy + Ord + Eq + Send + Sync;
    fn network(net: Self::Net) -> Self;
    fn broadcast(net: Self::Net) -> Self;
    fn prefix_len(net: Self::Net) -> u8;
    fn get_ipnet(net: Self::Net) -> IpNet;
    fn max_val() -> Self;
    fn add_one(v: Self) -> Self;
    fn sub_one(v: Self) -> Self;
    fn range_to_cidrs(start: Self, end: Self) -> Vec<Self::Net>;
}

impl SweepableIp for u32 {
    type Net = Ipv4Net;
    fn network(net: Self::Net) -> Self { net.network().into() }
    fn broadcast(net: Self::Net) -> Self { net.broadcast().into() }
    fn prefix_len(net: Self::Net) -> u8 { net.prefix_len() }
    fn get_ipnet(net: Self::Net) -> IpNet { IpNet::V4(net) }
    fn max_val() -> Self { u32::MAX }
    fn add_one(v: Self) -> Self { v + 1 }
    fn sub_one(v: Self) -> Self { v - 1 }
    fn range_to_cidrs(start: Self, end: Self) -> Vec<Self::Net> {
        let mut res = Vec::new();
        let mut s = start;
        while s <= end {
            let mut p = if s == 0 { 32 } else { s.trailing_zeros() };
            while p > 0 {
                let size = 1u64 << p;
                if size - 1 > (end as u64 - s as u64) { p -= 1; } else { break; }
            }
            res.push(Ipv4Net::new(s.into(), (32 - p) as u8).unwrap());
            let size = 1u64 << p;
            if size > (end as u64 - s as u64) { break; }
            s = s.wrapping_add(size as u32);
            if s == 0 { break; }
        }
        res
    }
}

impl SweepableIp for u128 {
    type Net = Ipv6Net;
    fn network(net: Self::Net) -> Self { net.network().into() }
    fn broadcast(net: Self::Net) -> Self { net.broadcast().into() }
    fn prefix_len(net: Self::Net) -> u8 { net.prefix_len() }
    fn get_ipnet(net: Self::Net) -> IpNet { IpNet::V6(net) }
    fn max_val() -> Self { u128::MAX }
    fn add_one(v: Self) -> Self { v + 1 }
    fn sub_one(v: Self) -> Self { v - 1 }
    fn range_to_cidrs(start: Self, end: Self) -> Vec<Self::Net> {
        let mut res = Vec::new();
        let mut s = start;
        loop {
            let mut p = if s == 0 { 128 } else { s.trailing_zeros() };
            while p > 0 {
                if p == 128 {
                    if s == 0 && end == u128::MAX { break; }
                    p = 127; continue;
                }
                let size = 1u128 << p;
                if size - 1 > (end - s) { p -= 1; } else { break; }
            }
            res.push(Ipv6Net::new(s.into(), (128 - p) as u8).unwrap());
            if p == 128 { break; }
            let size = 1u128 << p;
            if size > (end - s) { break; }
            s = s.wrapping_add(size);
            if s == 0 { break; }
        }
        res
    }
}
