public final class DriverBaseline {
    private DriverBaseline() {}

    public static int branch(int value) {
        if (value > 0) {
            return 7;
        }
        return 3;
    }

    public static int loop(int count) {
        int total = 0;
        for (int index = 0; index < count; index++) {
            total += index;
        }
        return total;
    }
}
