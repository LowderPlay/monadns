use std::collections::HashSet;
use ipnet::IpNet;
use nftables::schema::*;
use nftables::types::*;
use rtnetlink::{new_connection, IpVersion};
use futures::stream::TryStreamExt;
use std::net::{IpAddr};
use async_trait::async_trait;
use log::{info, error};
use nftables::batch::Batch;
use nftables::{expr, stmt};
use nftables::expr::{Expression, Meta, MetaKey, NamedExpression, Payload, PayloadField, TcpOption, Prefix};
use nftables::stmt::{Mangle, Match, Operator, Statement, NAT};
use rtnetlink::packet_route::rule::{RuleAction, RuleAttribute};
use crate::route_controller::RouteController;
use crate::config::InterfaceConfig;

const MAP_V4: &str = "fake_to_real_v4";
const MAP_V6: &str = "fake_to_real_v6";
const MAP_V4_MARK: &str = "fake_to_mark_v4";
const MAP_V6_MARK: &str = "fake_to_mark_v6";
const MAP_SUBNET_V4_MARK: &str = "subnet_to_mark_v4";
const MAP_SUBNET_V6_MARK: &str = "subnet_to_mark_v6";

#[derive(Clone)]
pub struct NetworkManager {
    interfaces: Vec<InterfaceConfig>,
    nft_table_name: String,
}

impl NetworkManager {
    pub fn new(interfaces: Vec<InterfaceConfig>) -> Self {
        Self {
            interfaces,
            nft_table_name: "monadns_steering".to_string(),
        }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        self.cleanup().await?;
        self.init_routing().await?;
        self.init_nftables()?;

        Ok(())
    }

    async fn init_routing(&self) -> anyhow::Result<()> {
        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        for iface in &self.interfaces {
            // Add Rule: fwmark -> table
            handle.rule().add().v4()
                .table_id(iface.table_id as u32)
                .fw_mark(iface.fwmark)
                .priority(100)
                .action(RuleAction::ToTable)
                .execute().await?;

            handle.rule().add().v6()
                .table_id(iface.table_id as u32)
                .fw_mark(iface.fwmark)
                .priority(100)
                .action(RuleAction::ToTable)
                .execute().await?;
        }

        Ok(())
    }

    fn init_nftables(&self) -> anyhow::Result<()> {
        let mut batch = Batch::new();
        let family = NfFamily::INet;

        batch.add(NfListObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        }));

        // Flush just in case
        batch.add_cmd(NfCmd::Flush(FlushObject::Table(Table {
            family,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let maps = [
            (MAP_V4, SetTypeValue::Single(SetType::Ipv4Addr), SetTypeValue::Single(SetType::Ipv4Addr), None),
            (MAP_V6, SetTypeValue::Single(SetType::Ipv6Addr), SetTypeValue::Single(SetType::Ipv6Addr), None),
            (MAP_V4_MARK, SetTypeValue::Single(SetType::Ipv4Addr), SetTypeValue::Single(SetType::Mark), None),
            (MAP_V6_MARK, SetTypeValue::Single(SetType::Ipv6Addr), SetTypeValue::Single(SetType::Mark), None),
            (MAP_SUBNET_V4_MARK, SetTypeValue::Single(SetType::Ipv4Addr), SetTypeValue::Single(SetType::Mark), Some(HashSet::from([SetFlag::Interval]))),
            (MAP_SUBNET_V6_MARK, SetTypeValue::Single(SetType::Ipv6Addr), SetTypeValue::Single(SetType::Mark), Some(HashSet::from([SetFlag::Interval]))),
        ];

        for (name, key, value, flags) in maps {
            batch.add(NfListObject::Map(Map {
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                set_type: key,
                map: value,
                flags,
                ..Default::default()
            }.into()));
        }

        let counters = ["cnt_v4_tx", "cnt_v4_rx", "cnt_v6_tx", "cnt_v6_rx"];
        for name in counters {
            for iface in &self.interfaces {
                batch.add(NfListObject::Counter(Counter {
                    family,
                    table: self.nft_table_name.clone().into(),
                    name: format!("{}_{}", name, iface.name).into(),
                    ..Default::default()
                }));
            }
        }

        batch.add(NfListObject::Counter(Counter {
            family,
            table: self.nft_table_name.clone().into(),
            name: "cnt_steering_subnet".into(),
            ..Default::default()
        }));
        batch.add(NfListObject::Counter(Counter {
            family,
            table: self.nft_table_name.clone().into(),
            name: "cnt_steering_fakeip".into(),
            ..Default::default()
        }));

        self.add_chains(&mut batch, family);

        // MTU clamping to avoid fragmentation issues on tunnels
        for iface in &self.interfaces {
            if let Some(mss) = iface.tcp_mss_clamp {
                batch.add(NfListObject::Rule(self.get_mtu_clamp_rule("forward", iface.fwmark, mss)));
                batch.add(NfListObject::Rule(self.get_mtu_clamp_rule("output", iface.fwmark, mss)));
            }
        }

        for ip in [IpVersion::V4, IpVersion::V6] {
            let (protocol, subnet_map) = match ip {
                IpVersion::V4 => ("ip", MAP_SUBNET_V4_MARK),
                IpVersion::V6 => ("ip6", MAP_SUBNET_V6_MARK),
            };
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_prerouting", protocol, subnet_map, subnet_map, "cnt_steering_subnet")));
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_output", protocol, subnet_map, subnet_map, "cnt_steering_subnet")));

            let (protocol, map_name, mark_map_name) = match ip {
                IpVersion::V4 => ("ip", MAP_V4, MAP_V4_MARK),
                IpVersion::V6 => ("ip6", MAP_V6, MAP_V6_MARK),
            };
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_prerouting", protocol, map_name, mark_map_name, "cnt_steering_fakeip")));
            batch.add(NfListObject::Rule(self.get_steering_rule("mangle_output", protocol, map_name, mark_map_name, "cnt_steering_fakeip")));

            for iface in &self.interfaces {
                batch.add(NfListObject::Rule(self.get_tx_interface_metrics_rule("mangle_prerouting", ip.clone(), iface)));
                batch.add(NfListObject::Rule(self.get_tx_interface_metrics_rule("mangle_output", ip.clone(), iface)));
            }

            for iface in &self.interfaces {
                batch.add(NfListObject::Rule(self.get_rx_interface_metrics_rule("mangle_prerouting", ip.clone(), iface)));
            }

            batch.add(NfListObject::Rule(self.get_dnat_rule("prerouting", ip.clone())));
            batch.add(NfListObject::Rule(self.get_dnat_rule("output", ip.clone())));
        }

        self.add_postrouting_rules(&mut batch, family);

        nftables::helper::apply_ruleset(&batch.to_nftables())?;

        Ok(())
    }

    fn add_chains(&self, batch: &mut Batch, family: NfFamily) {
        let chains = [
            (Some(NfChainType::Filter), Some(NfHook::Prerouting), "mangle_prerouting", -150, None),
            (Some(NfChainType::Route), Some(NfHook::Output), "mangle_output", -150, None),
            (Some(NfChainType::NAT), Some(NfHook::Prerouting), "prerouting", -100, None),
            (Some(NfChainType::NAT), Some(NfHook::Output), "output", -100, None),
            (Some(NfChainType::NAT), Some(NfHook::Postrouting), "postrouting", 100, None),
            (Some(NfChainType::Filter), Some(NfHook::Forward), "forward", 100, Some(NfChainPolicy::Accept)),
        ];

        for (ctype, hook, name, prio, policy) in chains {
            batch.add(NfListObject::Chain(Chain {
                _type: ctype,
                family,
                table: self.nft_table_name.clone().into(),
                name: name.into(),
                hook,
                prio: Some(prio),
                policy,
                ..Default::default()
            }));
        }
    }

    fn match_nfproto(nfproto: &str) -> Statement<'_> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Nfproto })),
            right: Expression::String(nfproto.into()),
            op: Operator::EQ,
        })
    }

    fn add_postrouting_rules(&self, batch: &mut Batch, family: NfFamily) {
        for iface in &self.interfaces {
            let fwmark_match = Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                right: Expression::Number(iface.fwmark),
                op: Operator::EQ,
            });
            let iface_match = Statement::Match(Match {
                left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Oifname })),
                right: Expression::String(iface.name.clone().into()),
                op: Operator::EQ,
            });

            let stacks = [
                (iface.ipv4_snat, "ip", "ipv4"),
                (iface.ipv6_snat, "ip6", "ipv6")
            ];

            for (snat, protocol, nfproto) in stacks {
                let mut rules = vec![
                    Self::match_nfproto(nfproto),
                    fwmark_match.clone(),
                    iface_match.clone()
                ];
                if let Some(snat) = snat {
                    rules.extend(vec![
                        Statement::Match(Match {
                            left: Expression::Named(NamedExpression::Payload(Payload::PayloadField(PayloadField {
                                protocol: protocol.into(),
                                field: "saddr".into(),
                            }))),
                            op: Operator::NEQ,
                            right: Expression::String(snat.to_string().into()),
                        }),
                        Statement::SNAT(Some(NAT {
                            addr: Some(Expression::String(snat.to_string().into())),
                            family: None,
                            port: None,
                            flags: None,
                        }))
                    ]);
                } else {
                    rules.push(Statement::Masquerade(None));
                }

                batch.add(NfListObject::Rule(Rule {
                    family,
                    table: self.nft_table_name.clone().into(),
                    chain: "postrouting".into(),
                    expr: rules.into(),
                    ..Default::default()
                }));
            }
        }
    }

    fn get_mtu_clamp_rule(&self, chain: &'static str, fwmark: u32, mtu: u32) -> Rule<'_> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(
                        NamedExpression::Payload(Payload::PayloadField(PayloadField {
                            protocol: "tcp".into(), field: "flags".into()
                        }))),
                    op: Operator::EQ,
                    right: Expression::String("syn".into()),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::TcpOption(TcpOption { name: "maxseg".into(), field: Some("size".into()) })),
                    value: Expression::Number(mtu),
                }),
            ].into(),
            ..Default::default()
        }
    }

    fn dest_match_statement<'a>(protocol: &'a str, map_name: &'a str) -> Statement<'a> {
        Statement::Match(Match {
            left: Expression::Named(NamedExpression::Payload(
                Payload::PayloadField(PayloadField {
                    protocol: protocol.into(),
                    field: "daddr".into()
                }))),
            right: Expression::String(format!("@{}", map_name).into()),
            op: Operator::EQ,
        })
    }

    fn map_expression<'a>(protocol: &'a str, map_name: &'a str) -> Expression<'a> {
        Expression::Named(NamedExpression::Map(Box::new(expr::Map {
            key: Expression::Named(NamedExpression::Payload(
                Payload::PayloadField(PayloadField {
                    protocol: protocol.into(),
                    field: "daddr".into()
                }))),
            data: Expression::String(format!("@{}", map_name).into()),
        })))
    }

    fn get_steering_rule(&self, chain: &'static str, protocol: &'static str, map_name: &'static str, mark_map_name: &'static str, counter_name: &'static str) -> Rule<'_> {
        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::dest_match_statement(protocol, map_name),
                Statement::Counter(stmt::Counter::Named(counter_name.into())),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    value: Self::map_expression(protocol, mark_map_name),
                }),
                Statement::Mangle(Mangle {
                    key: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "mark".into(),
                        ..Default::default()
                    })),
                    value: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                }),
            ].into(),
            ..Default::default()
        }
    }

    fn get_tx_interface_metrics_rule(&self, chain: &'static str, version: IpVersion, iface: &InterfaceConfig) -> Rule<'_> {
        let counter_base = match version {
            IpVersion::V4 => "cnt_v4_tx",
            IpVersion::V6 => "cnt_v6_tx",
        };
        let nfproto = match version {
            IpVersion::V4 => "ipv4",
            IpVersion::V6 => "ipv6",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::match_nfproto(nfproto),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::Meta(Meta { key: MetaKey::Mark })),
                    right: Expression::Number(iface.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Counter(stmt::Counter::Named(format!("{}_{}", counter_base, iface.name).into())),
            ].into(),
            ..Default::default()
        }
    }

    fn get_rx_interface_metrics_rule(&self, chain: &'static str, version: IpVersion, iface: &InterfaceConfig) -> Rule<'_> {
        let counter_base = match version {
            IpVersion::V4 => "cnt_v4_rx",
            IpVersion::V6 => "cnt_v6_rx",
        };
        let nfproto = match version {
            IpVersion::V4 => "ipv4",
            IpVersion::V6 => "ipv6",
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::match_nfproto(nfproto),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "mark".into(),
                        ..Default::default()
                    })),
                    right: Expression::Number(iface.fwmark),
                    op: Operator::EQ,
                }),
                Statement::Match(Match {
                    left: Expression::Named(NamedExpression::CT(expr::CT {
                        key: "direction".into(),
                        ..Default::default()
                    })),
                    right: Expression::String("reply".into()),
                    op: Operator::EQ,
                }),
                Statement::Counter(stmt::Counter::Named(format!("{}_{}", counter_base, iface.name).into())),
            ].into(),
            ..Default::default()
        }
    }

    fn get_dnat_rule(&self, chain: &'static str, version: IpVersion) -> Rule<'_> {
        let (protocol, map_name) = match version {
            IpVersion::V4 => ("ip", MAP_V4),
            IpVersion::V6 => ("ip6", MAP_V6),
        };

        Rule {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            chain: chain.into(),
            expr: vec![
                Self::dest_match_statement(protocol, map_name),
                Statement::DNAT(Some(NAT {
                    addr: Some(Self::map_expression(protocol, map_name)),
                    family: None,
                    port: None,
                    flags: None,
                }))
            ].into(),
            ..Default::default()
        }
    }
}

#[async_trait]
impl RouteController for NetworkManager {
    async fn add_mapping(&self, fake_ip: IpAddr, real_ip: IpAddr, fwmark: u32) -> anyhow::Result<()> {
        let (map_name, mark_map_name) = match fake_ip {
            IpAddr::V4(_) => (MAP_V4, MAP_V4_MARK),
            IpAddr::V6(_) => (MAP_V6, MAP_V6_MARK),
        };

        let mut batch = Batch::new();
        // Remove existing mapping for this fake_ip from BOTH maps
        for m in [map_name, mark_map_name] {
            batch.delete(NfListObject::Element(Element {
                family: NfFamily::INet,
                table: self.nft_table_name.clone().into(),
                name: m.into(),
                elem: vec![Expression::String(fake_ip.to_string().into())].into(),
            }));
        }

        if let Ok(_) = nftables::helper::apply_ruleset(&batch.to_nftables()) {
            info!("removed conflicting map entries for {}", fake_ip);
        }

        let mut batch = Batch::new();
        // Add to fake_to_real map
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: map_name.into(),
            elem: vec![Expression::List(vec![
                Expression::String(fake_ip.to_string().into()),
                Expression::String(real_ip.to_string().into()),
            ])].into(),
        }));

        // Add to fake_to_mark map
        batch.add(NfListObject::Element(Element {
            family: NfFamily::INet,
            table: self.nft_table_name.clone().into(),
            name: mark_map_name.into(),
            elem: vec![Expression::List(vec![
                Expression::String(fake_ip.to_string().into()),
                Expression::Number(fwmark),
            ])].into(),
        }));

        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        metrics::counter!("mapped_ip_count", "family" => if real_ip.is_ipv4() { "ipv4" } else { "ipv6" }).increment(1);
        Ok(())
    }
    async fn cleanup(&self) -> anyhow::Result<()> {
        let mut batch = Batch::new();

        batch.add_cmd(NfCmd::Delete(NfListObject::Table(Table {
            family: NfFamily::INet,
            name: self.nft_table_name.clone().into(),
            ..Default::default()
        })));

        let _ = nftables::helper::apply_ruleset(&batch.to_nftables());

        let (conn, handle, _) = new_connection()?;
        tokio::spawn(conn);

        for version in [IpVersion::V4, IpVersion::V6] {
            let mut rules = handle.rule().get(version).execute();
            while let Some(rule) = rules.try_next().await? {
                // Check if this rule's table_id and fwmark match any of our interfaces
                if let Some(mark) = rule.attributes.iter().find_map(|attr| match attr {
                    RuleAttribute::FwMark(m) => Some(*m),
                    _ => None
                }) {
                    if self.interfaces.iter().any(|iface| iface.fwmark == mark && iface.table_id == rule.header.table) {
                        handle.rule().del(rule).execute().await?;
                    }
                }
            }
        }

        Ok(())
    }
    async fn fetch_metrics(&self) -> anyhow::Result<()> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct NftOutput {
            nftables: Vec<std::collections::HashMap<String, serde_json::Value>>,
        }

        #[derive(Deserialize)]
        struct CounterData {
            table: String,
            name: String,
            packets: u64,
            bytes: u64,
        }

        let raw_json = nftables::helper::get_current_ruleset_raw(
            nftables::helper::DEFAULT_NFT,
            &["list", "counters"]
        )?;

        let output: NftOutput = serde_json::from_str(&raw_json)?;

        for obj in output.nftables {
            if let Some(counter_val) = obj.get("counter") {
                let c: CounterData = serde_json::from_value(counter_val.clone())?;
                if c.table == self.nft_table_name {
                    if c.name == "cnt_steering_subnet" {
                        metrics::gauge!("steering_hits", "type" => "subnet").set(c.packets as f64);
                        metrics::gauge!("steering_bytes", "type" => "subnet").set(c.bytes as f64);
                        continue;
                    }
                    if c.name == "cnt_steering_fakeip" {
                        metrics::gauge!("steering_hits", "type" => "fakeip").set(c.packets as f64);
                        metrics::gauge!("steering_bytes", "type" => "fakeip").set(c.bytes as f64);
                        continue;
                    }

                    let (family, direction, interface) = if c.name.starts_with("cnt_v4_tx") {
                        ("ipv4", "tx", if c.name.len() > 9 { &c.name[10..] } else { "total" })
                    } else if c.name.starts_with("cnt_v4_rx") {
                        ("ipv4", "rx", if c.name.len() > 9 { &c.name[10..] } else { "total" })
                    } else if c.name.starts_with("cnt_v6_tx") {
                        ("ipv6", "tx", if c.name.len() > 9 { &c.name[10..] } else { "total" })
                    } else if c.name.starts_with("cnt_v6_rx") {
                        ("ipv6", "rx", if c.name.len() > 9 { &c.name[10..] } else { "total" })
                    } else {
                        continue;
                    };

                    metrics::gauge!("intercepted_packets", "family" => family, "direction" => direction, "interface" => interface.to_string()).set(c.packets as f64);
                    metrics::gauge!("intercepted_bytes", "family" => family, "direction" => direction, "interface" => interface.to_string()).set(c.bytes as f64);
                }
            }
        }
        Ok(())
    }

    async fn sync_subnets(&self, subnets: Vec<(String, u32)>) -> anyhow::Result<()> {
        let mut v4_elements = Vec::new();
        let mut v6_elements = Vec::new();

        for (subnet_str, fwmark) in subnets {
            let net: IpNet = match subnet_str.parse() {
                Ok(n) => n,
                Err(_) => {
                    match subnet_str.parse::<IpAddr>() {
                        Ok(ip) => IpNet::new(ip, if ip.is_ipv4() { 32 } else { 128 })?,
                        Err(e) => {
                            error!("Failed to parse subnet '{}': {}", subnet_str, e);
                            continue;
                        }
                    }
                }
            };

            let prefix = Expression::Named(NamedExpression::Prefix(Prefix {
                addr: Box::new(Expression::String(net.network().to_string().into())),
                len: net.prefix_len() as u32,
            }));

            let element = Expression::List(vec![
                prefix,
                Expression::Number(fwmark),
            ]);

            if net.addr().is_ipv4() {
                v4_elements.push(element);
            } else {
                v6_elements.push(element);
            }
        }

        let mut batch = Batch::new();

        // Flush both maps first
        for map_name in [MAP_SUBNET_V4_MARK, MAP_SUBNET_V6_MARK] {
            batch.add_cmd(NfCmd::Flush(FlushObject::Map(Box::new(Map {
                family: NfFamily::INet,
                table: self.nft_table_name.clone().into(),
                name: map_name.into(),
                ..Default::default()
            }))));
        }

        for (elements, map) in [(v4_elements, MAP_SUBNET_V4_MARK), (v6_elements, MAP_SUBNET_V6_MARK)] {
            if !elements.is_empty() {
                batch.add(NfListObject::Element(Element {
                    family: NfFamily::INet,
                    table: self.nft_table_name.clone().into(),
                    name: map.into(),
                    elem: elements.into(),
                }));
            }
        }

        nftables::helper::apply_ruleset(&batch.to_nftables())?;
        Ok(())
    }
}
