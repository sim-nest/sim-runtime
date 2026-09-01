public final class PublicForms {
    private static int field = 4;

    private PublicForms() {}

    public static int field() {
        field = field + 1;
        return field;
    }

    public static int array() {
        int[] values = new int[2];
        values[1] = 9;
        return values[1];
    }

    public static int allocation() {
        return new PublicForms() != null ? 1 : 0;
    }

    public static int call() {
        return helper(6);
    }

    private static int helper(int value) {
        return value + 1;
    }

    public static int handler() {
        try {
            return 1 / 0;
        } catch (ArithmeticException expected) {
            return 8;
        }
    }

    public static int concat() {
        return ("sim" + field).length();
    }

    public static int initialization() {
        return Initialized.value;
    }

    public static int monitor() {
        synchronized (PublicForms.class) {
            return 10;
        }
    }

    private static final class Initialized {
        static int value = 11;
    }
}
