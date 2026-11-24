package demo;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.GenericArrayType;
import java.lang.reflect.Method;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Type;
import java.lang.reflect.TypeVariable;
import java.lang.reflect.WildcardType;

/**
 * A bytecode client for the generic-reflection natives, so the assertions run through the
 * interpreter's dispatch and arrays rather than a hand-built native call.
 */
public class GenericProbe {
    public static Type superType(Class<?> c) {
        return c.getGenericSuperclass();
    }

    public static Type[] interfaces(Class<?> c) {
        return c.getGenericInterfaces();
    }

    public static TypeVariable<?>[] classParameters(Class<?> c) {
        return c.getTypeParameters();
    }

    public static Type fieldType(Class<?> c, String name) throws Exception {
        return c.getDeclaredField(name).getGenericType();
    }

    public static Type returnType(Class<?> c, String name, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredMethod(name, parameterTypes).getGenericReturnType();
    }

    public static Type[] parameterTypes(Class<?> c, String name, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredMethod(name, parameterTypes).getGenericParameterTypes();
    }

    public static TypeVariable<?>[] methodParameters(
            Class<?> c, String name, Class<?>... parameterTypes) throws Exception {
        return c.getDeclaredMethod(name, parameterTypes).getTypeParameters();
    }

    public static Type[] constructorParameterTypes(Class<?> c, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredConstructor(parameterTypes).getGenericParameterTypes();
    }

    public static TypeVariable<?>[] constructorParameters(Class<?> c, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredConstructor(parameterTypes).getTypeParameters();
    }

    public static Type[] bounds(TypeVariable<?> variable) {
        return variable.getBounds();
    }

    public static String variableName(TypeVariable<?> variable) {
        return variable.getName();
    }

    public static Object declaration(TypeVariable<?> variable) {
        return variable.getGenericDeclaration();
    }

    public static Type[] arguments(Type type) {
        return ((ParameterizedType) type).getActualTypeArguments();
    }

    public static Type raw(Type type) {
        return ((ParameterizedType) type).getRawType();
    }

    public static Type owner(Type type) {
        return ((ParameterizedType) type).getOwnerType();
    }

    public static Type upperBound(Type type) {
        return ((WildcardType) type).getUpperBounds()[0];
    }

    public static Type lowerBound(Type type) {
        return ((WildcardType) type).getLowerBounds()[0];
    }

    public static int lowerBoundCount(Type type) {
        return ((WildcardType) type).getLowerBounds().length;
    }

    public static Type component(Type type) {
        return ((GenericArrayType) type).getGenericComponentType();
    }

    public static String typeName(Type type) {
        return type.getTypeName();
    }

    public static String objectText(Object value) {
        return value.toString();
    }

    public static String runtimeClassName(Type type) {
        return type.getClass().getName();
    }

    public static Type firstInterface(Class<?> c) {
        return c.getGenericInterfaces()[0];
    }

    public static Method method(Class<?> c, String name, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredMethod(name, parameterTypes);
    }

    public static Constructor<?> constructor(Class<?> c, Class<?>... parameterTypes)
            throws Exception {
        return c.getDeclaredConstructor(parameterTypes);
    }

    public static Field field(Class<?> c, String name) throws Exception {
        return c.getDeclaredField(name);
    }
}
