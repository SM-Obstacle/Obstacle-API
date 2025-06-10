use actix_web::{Responder, body::MessageBody};

pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R, B> Responder for Either<L, R>
where
    B: MessageBody + 'static,
    L: Responder<Body = B>,
    R: Responder<Body = B>,
{
    type Body = B;

    fn respond_to(self, req: &actix_web::HttpRequest) -> actix_web::HttpResponse<Self::Body> {
        match self {
            Either::Left(l) => <L as Responder>::respond_to(l, req),
            Either::Right(r) => <R as Responder>::respond_to(r, req),
        }
    }
}
