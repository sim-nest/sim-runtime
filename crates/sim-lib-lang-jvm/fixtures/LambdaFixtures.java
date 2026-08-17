import java.io.Serializable;
import java.util.function.Function;
import java.util.function.IntSupplier;
import java.util.function.Supplier;
import java.util.function.ToIntFunction;

final class LambdaFixtures {
    interface Counter { int count(); }
    interface Named { String name(); }

    private final int value;

    LambdaFixtures() { this(3); }
    LambdaFixtures(int value) { this.value = value; }
    private int privateValue() { return value; }
    static int staticValue() { return 11; }

    static IntSupplier nonCapturingLambda() { return () -> 7; }
    static IntSupplier capturingLambda(int captured) { return () -> captured; }
    static IntSupplier staticReference() { return LambdaFixtures::staticValue; }
    IntSupplier boundVirtualReference() { return this::privateValue; }
    static ToIntFunction<String> unboundVirtualReference() { return String::length; }
    static Supplier<LambdaFixtures> constructorReference() { return LambdaFixtures::new; }
    static Function<Named, String> interfaceReference() { return Named::name; }
    static Counter serializableAlternate() { return (Counter & Serializable) () -> 13; }
}
