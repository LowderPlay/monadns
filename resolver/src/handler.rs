use std::net::IpAddr;
use std::sync::Arc;
use arc_swap::ArcSwap;
use hickory_resolver::TokioResolver;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_proto::op::{Header, ResponseCode};
use hickory_server::authority::MessageResponseBuilder;
use hickory_proto::ProtoErrorKind;
use hickory_proto::rr::{rdata, RData};
use log::{debug, error};
use crate::domain_controller::DomainController;
use crate::fake_ip::IpManager;
use crate::route_controller::RouteController;

pub struct HandlerState {
    pub v4: IpManager,
    pub v6: IpManager,
    pub upstream: TokioResolver,
    pub domain_controller: Arc<dyn DomainController>,
    pub route_controller: Arc<dyn RouteController>,
}

#[derive(Clone)]
pub struct FakeIpHandler {
    pub state: Arc<ArcSwap<HandlerState>>,
}

impl FakeIpHandler {
    pub fn new(state: HandlerState) -> Self {
        Self {
            state: Arc::new(ArcSwap::from(Arc::new(state))),
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for FakeIpHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let state = self.state.load();
        let query = match request.queries() {
            [q] => q,
            _ => unimplemented!("one query only is supported"),
        };
        let name = query.name().to_string();
        let query_type = query.query_type().to_string();
        let mut header = Header::response_from_request(request.header());
        let builder = MessageResponseBuilder::from_message_request(request);
        debug!("query: [{}] {}", query.query_type(), name);

        let lookup = match state.upstream.lookup(&name, query.query_type()).await {
            Ok(lookup) => lookup,
            Err(e) => {
                let code = match e.kind() {
                    ProtoErrorKind::NoRecordsFound(hickory_proto::NoRecords {
                                                       response_code: ResponseCode::NXDomain, ..
                                                   }) => ResponseCode::NXDomain,
                    ProtoErrorKind::NoRecordsFound(_) => ResponseCode::NoError,
                    _ => ResponseCode::ServFail
                };
                header.set_response_code(code);

                metrics::counter!("dns_queries_total", "type" => query_type, "intercepted" => "false", "response_code" => code.to_string()).increment(1);
                return response_handle.send_response(builder.build_no_records(header)).await.unwrap();
            }
        };

        if state.domain_controller.should_intercept(&name).await {
            let mut records = lookup.records().to_vec();
            for r in &mut records {
                let real_ip = match r.data() {
                    RData::A(a) => state.v4.get_or_assign_ip(&IpAddr::V4(a.0)).await,
                    RData::AAAA(aaaa) => state.v6.get_or_assign_ip(&IpAddr::V6(aaaa.0)).await,
                    _ => continue
                };
                
                match real_ip {
                    Ok(IpAddr::V4(v4)) => r.set_data(RData::A(rdata::A(v4))),
                    Ok(IpAddr::V6(v6)) => r.set_data(RData::AAAA(rdata::AAAA(v6))),
                    Err(e) => {
                        error!("failed to assign ip {}", e);
                        header.set_response_code(ResponseCode::ServFail);
                        metrics::counter!("dns_queries_total", "type" => query_type, "intercepted" => "true", "response_code" => "Server Failure").increment(1);
                        return response_handle.send_response(builder.build_no_records(header)).await.unwrap();
                    }
                }.set_ttl(60);
            }

            let response = builder.build(header, &records, [], [], []);
            metrics::counter!("dns_queries_total", "type" => query_type, "intercepted" => "true", "response_code" => "No Error").increment(1);
            return response_handle.send_response(response).await.unwrap();
        }

        metrics::counter!("dns_queries_total", "type" => query_type, "intercepted" => "false", "response_code" => "No Error").increment(1);
        response_handle.send_response(builder
            .build(header, lookup.records(), [], [], [])).await.unwrap()
    }
}
