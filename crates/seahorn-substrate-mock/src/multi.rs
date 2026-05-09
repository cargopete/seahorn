use futures::{Stream, stream};
use seahorn_core::{Cursor, Substrate, SubstrateEvent};

use crate::{JupiterV6MockSubstrate, PumpfunMockSubstrate, RaydiumClmmMockSubstrate};

type BoxStream<'a> =
    std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<SubstrateEvent>> + Send + 'a>>;

/// Mock substrate that interleaves events from Pump.fun, Raydium CLMM, and Jupiter v6.
///
/// Use with `--mock --all` so that the `MultiHandler` sees events from all three programs.
/// Events arrive in round-robin order driven by whichever sub-stream fires next.
pub struct AllProgramsMockSubstrate {
    pumpfun: PumpfunMockSubstrate,
    raydium: RaydiumClmmMockSubstrate,
    jupiter: JupiterV6MockSubstrate,
}

impl Default for AllProgramsMockSubstrate {
    fn default() -> Self {
        Self {
            pumpfun: PumpfunMockSubstrate::default(),
            raydium: RaydiumClmmMockSubstrate::default(),
            jupiter: JupiterV6MockSubstrate::default(),
        }
    }
}

impl Substrate for AllProgramsMockSubstrate {
    fn stream(
        &self,
        _from: Option<Cursor>,
    ) -> impl Stream<Item = anyhow::Result<SubstrateEvent>> + Send + '_ {
        let streams: Vec<BoxStream<'_>> = vec![
            Box::pin(self.pumpfun.stream(None)),
            Box::pin(self.raydium.stream(None)),
            Box::pin(self.jupiter.stream(None)),
        ];
        stream::select_all(streams)
    }
}
