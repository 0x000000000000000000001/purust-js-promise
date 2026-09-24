module Test.Main where

import Prelude

import Data.Either (Either(..))
import Data.Int (toNumber)
import Data.Maybe (Maybe(..))
import Data.Newtype (unwrap)
import Data.Time.Duration (Milliseconds(..))
import Effect (Effect)
import Effect.Aff (Aff, delay, launchAff_, try)
import Effect.Class (liftEffect)
import Effect.Console (log)
import Effect.Exception (Error, error)
import Effect.Ref as Ref
import Effect.Uncurried (EffectFn1, mkEffectFn2, runEffectFn1)
import Promise as P
import Promise.Aff (toAff, toAffE)
import Promise.Internal as PI
import Promise.Lazy as Lazy
import Promise.Rejection as Rejection
import Test.Assert (assert)

main :: Effect Unit
main = launchAff_ runTests

-- | A pending promise that settles after `ms`, like the original suite's
-- | `setTimeout` helpers: aggregation runs while the promises are still
-- | pending, instead of seeing promises that are already settled. The internal
-- | constructor is used because the public `new` carries a `Flatten`
-- | constraint that cannot be solved for a polymorphic helper.
delayResolve :: forall a. Int -> a -> Effect (P.Promise a)
delayResolve ms value = do
  slot <- Ref.new (Nothing :: Maybe (EffectFn1 a Unit))
  promise <- runEffectFn1 PI.new $ mkEffectFn2 \onResolve _ -> Ref.write (Just onResolve) slot
  launchAff_ do
    delay (Milliseconds (toNumber ms))
    mResolve <- liftEffect (Ref.read slot)
    case mResolve of
      Just onResolve -> liftEffect (runEffectFn1 onResolve value)
      Nothing -> pure unit
  pure promise

delayReject :: Int -> Effect (P.Promise Int)
delayReject ms = do
  slot <- Ref.new (Nothing :: Maybe (EffectFn1 P.Rejection Unit))
  promise <- runEffectFn1 PI.new $ mkEffectFn2 \_ onReject -> Ref.write (Just onReject) slot
  launchAff_ do
    delay (Milliseconds (toNumber ms))
    mReject <- liftEffect (Ref.read slot)
    case mReject of
      Just onReject ->
        liftEffect (runEffectFn1 onReject (Rejection.fromError (error ("rejected after " <> show ms <> " ms"))))
      Nothing -> pure unit
  pure promise

runTests :: Aff Unit
runTests = do
  liftEffect $ log "Testing resolve and then_"

  resolved <- toAff (P.resolve 42)
  liftEffect $ assert (resolved == 42)

  chained <- toAffE (P.then_ (\n -> pure (P.resolve (n * 2))) (P.resolve 21))
  liftEffect $ assert (chained == 42)

  success <- toAffE (P.thenOrCatch (\n -> pure (P.resolve (n + 1))) (\_ -> pure (P.resolve 0)) (P.resolve 41))
  liftEffect $ assert (success == 42)

  liftEffect $ log "Testing rejection handling"

  recovered <- toAffE (P.catch (\_ -> pure (P.resolve "caught")) (P.reject (Rejection.fromError (error "boom"))))
  liftEffect $ assert (recovered == "caught")

  fallback <- toAffE (P.thenOrCatch (\n -> pure (P.resolve (n + 1))) (\_ -> pure (P.resolve 7)) (P.reject (Rejection.fromError (error "nope"))))
  liftEffect $ assert (fallback == 7)

  outcome <- try (toAff (P.reject (Rejection.fromError (error "expected"))))
  liftEffect $ checkRejection outcome

  liftEffect $ log "Testing all and race"

  allValues <- toAffE (P.all [ P.resolve 1, P.resolve 2, P.resolve 3 ])
  liftEffect $ assert (allValues == [ 1, 2, 3 ])

  raced <- toAffE (P.race [ P.resolve "fast", P.resolve "slow" ])
  liftEffect $ assert (raced == "fast")

  -- `all` must reject with the first failure while the other input is still
  -- pending, then its later resolution must stay harmless.
  allStaysPending <- liftEffect (delayResolve 100 1)
  allFailure <- liftEffect (delayReject 20)
  allRejection <- try (toAffE (P.all [ allStaysPending, allFailure ]))
  liftEffect $ checkRejection allRejection

  -- A real race between promises that settle later: the winner is the one
  -- that settles first, not the one that already was resolved.
  raceLoser <- liftEffect (delayReject 500)
  raceWinnerPromise <- liftEffect (delayResolve 10 42)
  raceWinner <- toAffE (P.race [ raceLoser, raceWinnerPromise ])
  liftEffect $ assert (raceWinner == 42)

  liftEffect $ log "Testing finally and the executor constructor"

  finalizerRan <- liftEffect (Ref.new false)
  finallyTest finalizerRan

  -- The finalizer runs for rejections too, and the rejection is preserved.
  rejectedFinalizer <- liftEffect (Ref.new false)
  rejectedOutcome <- try (toAffE (P.finally (finalizer rejectedFinalizer) (P.reject (Rejection.fromError (error "finally-reject")))))
  liftEffect do
    checkRejection rejectedOutcome
    rejectedFinalizerDone <- Ref.read rejectedFinalizer
    assert rejectedFinalizerDone

  fromExecutor <- toAffE (P.new (executor 11))
  liftEffect $ assert (fromExecutor == 11)

  liftEffect $ log "Testing LazyPromise"

  lazyDirect <- toAffE (unwrap (Lazy.new (lazyExecutor 5)))
  let Lazy.Box directValue = lazyDirect
  liftEffect $ assert (directValue == 5)

  lazyMonad <- toAffE (unwrap lazySum)
  let Lazy.Box monadValue = lazyMonad
  liftEffect $ assert (monadValue == 42)

  lazyChained <- toAffE (unwrap lazyChain)
  let Lazy.Box chainedValue = lazyChained
  liftEffect $ assert (chainedValue == 30)

  lazyCaught <- toAffE (unwrap (Lazy.catch (\_ -> pure 99) (Lazy.fromPromise (delayReject 10))))
  let Lazy.Box caughtValue = lazyCaught
  liftEffect $ assert (caughtValue == 99)

  lazyFinallyRef <- liftEffect (Ref.new 0)
  lazyFinally <- toAffE (unwrap (Lazy.finally (Lazy.fromPromise (finalizerCount lazyFinallyRef)) (pure "ok")))
  let Lazy.Box lazyFinallyValue = lazyFinally
  liftEffect do
    assert (lazyFinallyValue == "ok")
    lazyFinallyCount <- Ref.read lazyFinallyRef
    assert (lazyFinallyCount == 1)

  lazyAll <- toAffE (unwrap (Lazy.all [ pure 1, pure 2, pure 3 ]))
  let Lazy.Box lazyAllValues = lazyAll
  liftEffect $ assert (lazyAllValues == [ 1, 2, 3 ])

  liftEffect $ log "Tests passed"

checkRejection :: forall a. Either Error a -> Effect Unit
checkRejection = case _ of
  Left _ -> pure unit
  Right _ -> assert false

finallyTest :: Ref.Ref Boolean -> Aff Unit
finallyTest flag = do
  finalized <- toAffE (P.finally (finalizer flag) (P.resolve 9))
  finalizerDone <- liftEffect (Ref.read flag)
  liftEffect $ assert (finalized == 9)
  liftEffect $ assert finalizerDone

finalizer :: Ref.Ref Boolean -> Effect (P.Promise Unit)
finalizer flag = Ref.write true flag *> pure (P.resolve unit)

finalizerCount :: Ref.Ref Int -> Effect (P.Promise Unit)
finalizerCount flag = Ref.modify_ (_ + 1) flag *> pure (P.resolve unit)

executor :: Int -> P.Executor Int
executor value resolve' _reject' = resolve' value

lazyExecutor :: Int -> P.Executor Int
lazyExecutor value resolve' _reject' = resolve' value

lazySum :: Lazy.LazyPromise Int
lazySum = do
  a <- pure 20
  b <- pure 22
  pure (a + b)

lazyChain :: Lazy.LazyPromise Int
lazyChain = do
  v1 <- Lazy.new (\res _ -> res 10)
  v2 <- pure 20
  pure (v1 + v2)
