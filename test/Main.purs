module Test.Main where

import Prelude

import Data.Either (Either(..))
import Data.Newtype (unwrap)
import Effect (Effect)
import Effect.Aff (Aff, launchAff_, try)
import Effect.Class (liftEffect)
import Effect.Console (log)
import Effect.Exception (Error, error)
import Effect.Ref as Ref
import Promise as P
import Promise.Aff (toAff, toAffE)
import Promise.Lazy as Lazy
import Promise.Rejection as Rejection
import Test.Assert (assert)

main :: Effect Unit
main = launchAff_ runTests

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

  liftEffect $ log "Testing finally and the executor constructor"

  finalizerRan <- liftEffect (Ref.new false)
  finallyTest finalizerRan

  fromExecutor <- toAffE (P.new (executor 11))
  liftEffect $ assert (fromExecutor == 11)

  liftEffect $ log "Testing LazyPromise"

  lazyDirect <- toAffE (unwrap (Lazy.new (lazyExecutor 5)))
  let Lazy.Box directValue = lazyDirect
  liftEffect $ assert (directValue == 5)

  lazyMonad <- toAffE (unwrap lazySum)
  let Lazy.Box monadValue = lazyMonad
  liftEffect $ assert (monadValue == 42)

  liftEffect $ log "Tests passed"

checkRejection :: Either Error Int -> Effect Unit
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

executor :: Int -> P.Executor Int
executor value resolve' _reject' = resolve' value

lazyExecutor :: Int -> P.Executor Int
lazyExecutor value resolve' _reject' = resolve' value

lazySum :: Lazy.LazyPromise Int
lazySum = do
  a <- pure 20
  b <- pure 22
  pure (a + b)
